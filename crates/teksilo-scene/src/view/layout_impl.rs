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
        // Republish for the child paint nodes — a `SceneBandProxy` or a
        // `WetLayerNode` knows its own rect, not the viewport's, and a pan is a
        // relayout, so this is the earliest point in a moved frame at which it
        // can be right. See `view::paint_node`.
        self.published_visible_region.set(region);
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
        // The selection transform's live preview. This is the heavyweight half
        // of the controller's one preview affine — the lightweight tier
        // composes the SAME value into each item's `local → scene` in
        // `paint_band`. Two expressions of one function, so a mixed selection
        // cannot half-move mid-gesture.
        //
        // Applying it to the placement rectangle means a resize previews as a
        // real **relayout** here: the card is laid out at the new size and its
        // text re-wraps as the handle moves. The lightweight tier composes the
        // same affine as a visual scale instead, so the two agree on the box and
        // not on the interior until the commit — see the `transform_session`
        // module header.
        //
        // It reaches here at all because the controller's tick is a `Signal`
        // bound at `BindingLevel::Relayout`, not a plain `Cell`: `place_children`
        // re-runs on a relayout and on nothing weaker, so a repaint-only trigger
        // would have previewed the frame and the lightweight items while the
        // cards sat still until the commit.
        let transform_preview = self.transform_preview();
        // Sizes measured from the cards this pass lays out, written back to the
        // model **after** the shared borrow below is released. Collected rather
        // than written in place because `SceneModel`'s mutators take
        // `&self` and reach for a `borrow_mut` — inside this loop that is the
        // live shared borrow, and a `RefCell` does not forgive that.
        let mut measured: Vec<(ItemId, Size)> = Vec::new();
        let scene = self.model.0.borrow();
        for placement in children.iter_mut() {
            let Some(&item_id) = self.widget_to_item.get(&placement.id) else {
                // A paint node — an `Interleaved` item's `SceneBandProxy`, or
                // the wet layer. Placed here and left out of `at_children` on
                // purpose: the item it paints already has its synthetic AT node
                // from the scene's own walker, and publishing the node as well
                // would announce the same item twice (and announce a wet stroke
                // as an object, which it is not yet). This is the whole of the
                // accessibility cost of the `Interleaved` band: none.
                if let Some(rect) = self.paint_node_rect(placement.id, bounds) {
                    placement.origin = Point::new(rect.x, rect.y);
                    placement.size = Size::new(rect.width, rect.height);
                    // Culled on the **visible** region, not the retention one:
                    // a paint node holds no state worth keeping warm, and it
                    // paints its item at absolute scene coordinates without
                    // consulting the rect it is given — so a node left alive
                    // outside the viewport would keep painting geometry the
                    // clip throws away. The wet layer's rect *is* the viewport,
                    // so it never parks.
                    placement.dormant = !crate::scene::rects_intersect(rect, region);
                    continue;
                }
                // Not ours at all. The cull has no opinion about it, and
                // `at_children` is the whole accessibility child list, so
                // leaving it out would quietly hide it from AT rather than
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
            // …through the transform preview, when this card is one the live
            // gesture carries. Everything downstream — the cull, the retention
            // decision, the accessibility rectangle — then reads the position
            // the user can see rather than the one the model still holds.
            let previewing = match &transform_preview {
                Some((roots, _)) => roots
                    .iter()
                    .any(|r| *r == item_id || scene.is_descendant_of(item_id, *r)),
                None => false,
            };
            let rect = match (&transform_preview, previewing) {
                (Some((_, xform)), true) => xform.apply_rect(rect),
                _ => rect,
            };
            // **Content-driven size.** A non-`Fixed` policy hands one or both
            // axes to the widget, and this is the only place in the framework
            // that can ask: `place_children` runs *before* any child's own
            // layout (the recursion below lays each one out at
            // `SizeProposal::exact(placement.size)`), so a measurement here is
            // a pure query whose answer is what the child is then given.
            //
            // Only the cards this pass is laying out are measured. A card
            // outside the viewport keeps the size it was last measured at — or,
            // if it has never been on screen, the one `add_widget_item` was
            // handed. That is the same contract `ListView::auto_item_height`
            // makes about a row it has never realised, and it is what keeps the
            // cull's promise: measuring every card in the scene on every pass
            // is exactly the cost the cull exists to remove.
            let visible = crate::scene::rects_intersect(rect, region);
            let rect = if visible {
                match self.measure_entry(
                    placement.id,
                    item_id,
                    rect,
                    previewing,
                    ctx,
                    &mut measured,
                ) {
                    Some(sized) => sized,
                    None => rect,
                }
            } else {
                rect
            };
            placement.origin = Point::new(rect.x, rect.y);
            placement.size = if visible {
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
        // The model now agrees with what was placed, so the spatial index, a
        // marquee, `scene_rect_extent` and any **other** view of this model see
        // the height the user can see rather than the estimate. Each write
        // notifies, which is what wakes a sibling view; this one's own extra
        // pass measures the same height and writes nothing.
        for (item_id, size) in measured {
            self.model.set_measured_size(item_id, size);
        }
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

    /// The per-item measurement history, as a handle a `HandlerSet` closure or
    /// a change observer can own. See [`SceneView::measure_state`].
    pub(super) fn measure_state_handle(&self) -> Rc<RefCell<HashMap<ItemId, MeasureTrack>>> {
        Rc::clone(&self.measure_state)
    }

    /// Measure one card against its [`SizePolicy`](crate::SizePolicy) and
    /// return the rectangle it should actually be placed at, or `None` when the
    /// policy is [`Fixed`](crate::SizePolicy::Fixed) and there is nothing to
    /// ask.
    ///
    /// Anything worth writing back is pushed onto `out`; the caller applies it
    /// once the scene borrow is released.
    ///
    /// # Which query, and why it matters
    ///
    /// [`child_layout_response`](LayoutContext::child_layout_response) for a
    /// card the arena still considers active, and
    /// [`measure_intrinsic`](LayoutContext::measure_intrinsic) only as the
    /// fallback for one it does not — a card being woken this pass, whose
    /// dormancy is last pass's verdict.
    ///
    /// They are not interchangeable. `measure_intrinsic` sets the arena's
    /// `measuring` flag for the **whole subtree**, which bypasses the per-pass
    /// layout memo for every nested query underneath it. That memo is what
    /// makes the framework's main-then-cross negotiation linear; without it a
    /// deep stack inside a card re-asks the same children once per level. So
    /// the uncached door is the exception, taken for one pass per wake, and the
    /// memoized one is the rule.
    ///
    /// # `previewing` — a measurement can be right and still not be the model's
    ///
    /// `rect` is what the card is *placed* at, which during a live selection
    /// transform is the preview's rectangle and not the model's. Measuring
    /// against it is the point: that is what makes a resize reflow the words as
    /// the handle moves. Writing the answer back is not. The preview is
    /// recomputed from the gesture's frozen start frame on every sample, so a
    /// previewed width that reached the model would be scaled again on the next
    /// sample and again on the one after — ten samples of a 100-unit drag
    /// committed 759 units instead of 300.
    ///
    /// So a previewed pass measures, places, and tells neither the model nor
    /// the measurement history anything. The gesture stays what the
    /// [`transform_session`](crate::transform_session) module header says it is
    /// — one write, on release, with nothing to roll back when it is cancelled
    /// — and the measurement that follows the commit is taken against a width
    /// the model actually holds.
    ///
    /// # Who drove this pass
    ///
    /// The answer is written back unless the card contradicts itself on a pass
    /// that nothing outside this view drove — see [`MeasureTrack`], which owns
    /// that rule and the bound that goes with it. The question is answered by
    /// [`card_was_reached`], and asked lazily, because the two branches that
    /// decide without it are the ones taken on every ordinary pass.
    fn measure_entry(
        &self,
        widget_id: WidgetId,
        item_id: ItemId,
        rect: Rect,
        previewing: bool,
        ctx: &LayoutContext,
        out: &mut Vec<(ItemId, Size)>,
    ) -> Option<Rect> {
        let policy = self.model.0.borrow().size_policy(item_id);
        let proposal = match policy {
            crate::scene::SizePolicy::Fixed => return None,
            // Width authored, height asked for at that width.
            crate::scene::SizePolicy::HeightForWidth => SizeProposal {
                width: Some(rect.width),
                height: None,
            },
            // Shrink-wrap: the rect supplies only the position.
            crate::scene::SizePolicy::Intrinsic => SizeProposal {
                width: None,
                height: None,
            },
        };
        let measured = ctx
            .child_layout_response(widget_id, proposal)
            .map(|r| r.size)
            .or_else(|| ctx.measure_intrinsic(widget_id, proposal))?;
        let candidate = match policy {
            crate::scene::SizePolicy::HeightForWidth => {
                Size::new(rect.width, measured.height.max(0.0))
            }
            _ => Size::new(measured.width.max(0.0), measured.height.max(0.0)),
        };
        if previewing {
            // Placed at the answer, and that is all — see the `previewing`
            // section above.
            return Some(Rect::new(rect.x, rect.y, candidate.width, candidate.height));
        }
        let mut state = self.measure_state.borrow_mut();
        let track = state
            .entry(item_id)
            .or_insert_with(|| MeasureTrack::new(rect.width, candidate));
        let settled = track.resolve(rect.width, candidate, || {
            ctx.arena()
                .is_some_and(|arena| card_was_reached(arena, widget_id))
        });
        if settled != Size::new(rect.width, rect.height) {
            out.push((item_id, settled));
        }
        Some(Rect::new(rect.x, rect.y, settled.width, settled.height))
    }

    /// The scene rect for one of this view's own **paint nodes**, or `None`
    /// when `id` is not one.
    ///
    /// The rect is in scene coordinates, like every other child of a
    /// `SceneView`: the node lives inside the content transform, so this is
    /// what puts it where its content is. It is read only for culling and for
    /// the walker's own bookkeeping — both nodes paint at absolute scene
    /// coordinates and ignore the rect they are given, because applying it
    /// would place the content twice.
    ///
    /// - A [`SceneBandProxy`](super::paint_node::SceneBandProxy) takes its
    ///   item's scene rect, so an off-screen interleaved item culls exactly as
    ///   the card beside it would.
    /// - The wet layer takes the **viewport**, because what it paints is not in
    ///   the model and so has no rect the view could ask for. A wet stroke is
    ///   clipped to the viewport anyway (`SceneView::clips_children`), so a
    ///   larger rect would buy nothing.
    fn paint_node_rect(&self, id: WidgetId, bounds: Rect) -> Option<Rect> {
        if self.wet_node == Some(id) {
            return Some(self.visible_scene_region(bounds));
        }
        let item = self
            .interleaved_nodes
            .iter()
            .find(|(_, wid)| **wid == id)
            .map(|(item, _)| *item)?;
        // An entry whose geometry will not resolve still gets a rect: a
        // zero-size node at the origin would be culled, and a lightweight item
        // with no resolvable transform is a bug to surface, not to hide.
        Some(
            self.model
                .0
                .borrow()
                .scene_rect(item)
                .unwrap_or_else(|| self.visible_scene_region(bounds)),
        )
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

/// Whether anything but this view has reached `card` since it was last
/// measured — the whole of the discriminator [`MeasureTrack`] rests on.
///
/// `card` is the node the view hands to `child_layout_response`, i.e. the root
/// of the card's own subtree, and asking that one node is enough because of an
/// invariant the framework maintains: a `BindingLevel::Relayout` or `Rebuild`
/// binding calls `WidgetArena::mark_ancestors_need_layout`, so a content signal
/// firing three levels down inside a text engine marks every node up to and
/// past the card's root. The flag is therefore a question about the whole
/// subtree that costs one lookup. It is pinned from this side by
/// `an_edit_deep_inside_a_card_reaches_the_node_the_view_measures`; if the
/// framework ever stopped propagating, that test goes red rather than a card
/// silently freezing.
///
/// The converse is what makes it a *discriminator* rather than just a dirty
/// check: this view's own write marks the `SceneView` node and the view's
/// **ancestors**, and never a descendant — so on the pass a write causes, the
/// card reads clean. A sibling card's edit is equally invisible here, because
/// propagation goes up and not down.
///
/// Read during the view's own `place_children`, which is the only window in
/// which the answer exists: the flags are set before the walk starts (dirty
/// bindings are flushed in `process_state_changes`, a fresh node is born with
/// them set, a node parked dormant keeps them) and cleared for every active
/// node **after** the walk ends.
fn card_was_reached(arena: &teksilo_core::arena::WidgetArena, card: WidgetId) -> bool {
    arena.get(card).is_some_and(|node| node.dirty.needs_layout)
}
