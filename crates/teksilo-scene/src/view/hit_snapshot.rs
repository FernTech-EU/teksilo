// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The two hit-test snapshots, and the bookkeeping that keeps them current
//! without rebuilding them.
//!
//! [`SceneView`] cannot re-enter the scene's `RefCell` on the pointer's hot
//! path — an [`ItemChange`] observer may be mid-mutation — so dispatch reads
//! two per-view snapshots instead: every hit-testable entry (tap / hover /
//! cursor / context menu), and the draggable subset (the drag-start grab).
//! Both are sorted by [`PaintKey`] descending, so the first shape match is the
//! entry the user sees on top.
//!
//! # Why they are not rebuilt per pass
//!
//! Every field of both snapshots is a pure function of the **model**.
//! `scene_transform` walks only entry data; `scene_rect` is that transform
//! applied to `local_bounds`; the shape is the item's own; and the narrow phase
//! takes the view scale as a *call-time* argument. Nothing in either snapshot
//! depends on pan, zoom or rotation.
//!
//! That matters because the camera signals are bound at
//! [`BindingLevel::Relayout`], so
//! **a pan runs a full layout pass while touching no item at all**. Rebuilding
//! from `scene.ids()` there cost 16.1 ms per pan sample on a 50 000-item scene
//! whose visible population was 540 — more than a whole 120 Hz frame spent on
//! items nobody could see. Conditioning the rebuild on the viewport would not
//! have fixed it either: the right answer is not to rebuild at all.
//!
//! So the snapshots are built once and invalidated per item from the
//! `item_change_signal` stream, and a pan pays one `u64` comparison.
//!
//! # Why the invalidation is safe
//!
//! A cache over a notification stream is only as good as its proof that it saw
//! the whole stream, and this one does not assume. [`Scene`] counts the changes
//! it has **emitted** ([`Scene::item_change_version`]); this view counts the
//! ones its observer was **delivered**. [`HitSnapshotSync::plan`] patches only
//! when those two agree both now and at the previous refresh — so a change
//! emitted before this view's observer existed, one still queued inside an open
//! write scope, or one lost to an observer panic mid-drain, each resolve to a
//! full rebuild rather than to a stale row. Every doubt is a rebuild.
//!
//! The classification is the other half: only [`HitInvalidation::Geometry`]
//! takes the patch path, and it is the set of changes that provably move a row
//! without changing which rows exist or what order they are in.
//!
//! # Where the guarantee stops
//!
//! Everything in a row is reachable only through a [`Scene`] mutator, and every
//! one of those emits — there is no `item_mut` and no other door to a mounted
//! [`SceneItem`](crate::SceneItem). The boundary is an item that derives its own
//! `local_bounds` from something the scene cannot see (a `Signal`): it changes
//! shape while emitting nothing, so a cached row keeps the geometry it was built
//! with. That is what
//! [`Scene::add_item_dynamic`](crate::Scene::add_item_dynamic) is for — the
//! per-build re-read routes through `set_local_bounds` and therefore through the
//! change stream. A *static* entry whose bounds move behind the scene's back was
//! already outside the contract: its `SceneEntry::local_bounds` — the rectangle
//! the spatial index and every broad phase read — is snapshotted at insert.

use super::*;
use crate::scene::ItemChange;

/// What one [`ItemChange`] does to the cached snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HitInvalidation {
    /// Nothing either snapshot holds derives from it.
    ///
    /// Opacity is not a hit-test input (it never gates
    /// [`Scene::is_hit_testable`], which reads visibility and flags), and a
    /// heavyweight payload swap cannot reach a snapshot neither tier's widget
    /// entries appear in.
    None,
    /// The item's local→scene geometry, or its own shape, changed. Which rows
    /// exist and what order they sit in are both untouched, so the affected
    /// rows can be rewritten where they are.
    ///
    /// Carries its **subtree** with it: a parent's move shifts every
    /// descendant's `scene_transform`, and the change names only the parent.
    Geometry,
    /// Membership, paint order, or handlers changed — a row appears,
    /// disappears, moves in the sort, or carries a different handler set. Only
    /// a rebuild from the model can express that.
    Structure,
}

/// Which snapshot maintenance a refresh owes, decided by
/// [`HitSnapshotSync::plan`].
pub(super) enum SnapshotPlan {
    /// Nothing was emitted since the last refresh: the snapshots are already
    /// correct. The pan case, and the reason a pan is now free.
    Reuse,
    /// Rewrite these ids' rows (and their subtrees') in place.
    Patch(Vec<ItemId>),
    /// Rebuild both snapshots from the model.
    Rebuild,
}

/// How a change reaches the snapshots — the table the observer and the refresh
/// both read, so they cannot disagree about what a variant means.
pub(super) fn classify(change: &ItemChange) -> HitInvalidation {
    match change {
        // Local→scene geometry. `LocalBoundsChanged` also re-reads the item's
        // own shape, which is where a `PathItem` refitted to a new box lands.
        ItemChange::LocalPosChanged { .. }
        | ItemChange::TransformChanged { .. }
        | ItemChange::LocalBoundsChanged { .. }
        // A derived size is still a size. Heavyweight entries are in neither
        // snapshot, so the only entry this can reach is a lightweight one an
        // app measured itself — and for that the shape moved exactly as much
        // as a `set_local_bounds` would have moved it.
        | ItemChange::MeasuredSizeChanged { .. } => HitInvalidation::Geometry,
        // Paint-only by name, but a stroked `PathItem` derives its hit band
        // from the stroke it draws, so the shape moves with it.
        ItemChange::AppearanceChanged { .. } => HitInvalidation::Geometry,
        // A different item box at the same id: new `local_bounds`, new shape.
        // Geometry rather than Structure because membership is unchanged — the
        // row is still there, its silhouette is not.
        ItemChange::ItemReplaced { .. } => HitInvalidation::Geometry,
        // Membership: `is_hit_testable` chains visibility up the parent chain,
        // so a flag flip anywhere can add or drop a whole subtree.
        ItemChange::VisibilityChanged { .. }
        | ItemChange::FlagsChanged { .. }
        // Paint order: the sort key itself moves.
        | ItemChange::ZChanged { .. }
        | ItemChange::LayerChanged { .. }
        // Both at once — the subtree's geometry and its inherited visibility.
        | ItemChange::ParentChanged { .. }
        // Parent, z, position and transform at once: membership *and*
        // geometry, so the wider verdict is the only correct one.
        | ItemChange::PlacementChanged { .. }
        | ItemChange::Added { .. }
        | ItemChange::Removed { .. }
        // A snapshot row carries a clone of the handler set and the
        // `claims_press` verdict derived from it.
        | ItemChange::HandlersChanged { .. } => HitInvalidation::Structure,
        // Neither snapshot has an opacity field, and a widget entry is in
        // neither snapshot.
        // Neither snapshot has an opacity field, a widget entry is in neither,
        // and a size policy is not geometry — only the measurement it leads to
        // is, and that arrives as its own change.
        ItemChange::OpacityChanged { .. }
        | ItemChange::PayloadChanged { .. }
        | ItemChange::SizePolicyChanged { .. } => HitInvalidation::None,
    }
}

/// Per-view record of what has happened to the model since this view's
/// snapshots were last made current.
///
/// Lives behind an `Rc<RefCell<..>>` because the `item_change_signal` observer
/// writes it and the (`&self`) layout pass reads it.
#[derive(Debug, Default)]
pub(super) struct HitSnapshotSync {
    /// Ids whose geometry changed since the last refresh.
    dirty: HashSet<ItemId>,
    /// Set by any [`HitInvalidation::Structure`] change: the next refresh
    /// rebuilds rather than patches.
    rebuild_all: bool,
    /// Item changes delivered to this view's observer, ever. Its *delta* across
    /// a window is compared against the model's emission delta to prove the
    /// dirty set covers that window. Absolute equality would be the wrong test:
    /// every change emitted before this view's observer existed — the whole
    /// initial population — is a permanent offset between the two counters.
    seen: u64,
    /// The last refresh, or `None` until the first one: a view that has never
    /// built its snapshots cannot patch them.
    refreshed_at: Option<RefreshMark>,
    /// Which branch each refresh took. Test-only, and the only way a test can
    /// tell "the snapshots are correct" from "the snapshots are correct because
    /// they were rebuilt anyway" — which is the difference between a test that
    /// pins this mechanism and one that would pass with it deleted.
    #[cfg(test)]
    pub(super) tally: PlanTally,
}

/// Per-branch refresh counts. See [`HitSnapshotSync::tally`].
#[cfg(test)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct PlanTally {
    pub(super) reused: u64,
    pub(super) patched: u64,
    pub(super) rebuilt: u64,
}

/// Where the last refresh left the two counters, and whether the change queue
/// was drained at that moment.
#[derive(Debug, Clone, Copy)]
struct RefreshMark {
    /// [`Scene::item_change_version`] as the refresh read it.
    emitted: u64,
    /// [`HitSnapshotSync::seen`] at the same moment.
    seen: u64,
    /// Whether the scene's notification queue was empty then.
    ///
    /// Load-bearing, and not implied by the counters. With a change still
    /// queued at *both* ends of a window, the deliveries counted inside it can
    /// be the tail of an **older** batch while the window's own changes are
    /// still waiting — equal counts, wrong coverage. Requiring the queue to be
    /// drained at both ends makes the FIFO's counts mean what they look like.
    settled: bool,
}

impl HitSnapshotSync {
    /// Fold one delivered change into the record.
    pub(super) fn record(&mut self, change: &ItemChange) {
        self.seen = self.seen.wrapping_add(1);
        match classify(change) {
            HitInvalidation::None => {}
            HitInvalidation::Geometry => {
                self.dirty.insert(change.id());
            }
            HitInvalidation::Structure => {
                self.rebuild_all = true;
                // The rebuild reads the model whole, so anything collected for
                // a patch is now noise — and keeping it would grow without
                // bound across a run of structural changes.
                self.dirty.clear();
            }
        }
    }

    /// Decide what this refresh owes, and record that it happened.
    ///
    /// `emitted` is [`Scene::item_change_version`] and `settled` whether the
    /// scene's notification queue is drained, both read under the same borrow
    /// the refresh will use — so the answer describes the model the caller is
    /// about to read.
    pub(super) fn plan(&mut self, emitted: u64, settled: bool) -> SnapshotPlan {
        let Some(mark) = self.refreshed_at else {
            // Never built. Everything that happened before this view existed is
            // in the model already and in no dirty set.
            self.reset_to(emitted, settled);
            return SnapshotPlan::Rebuild;
        };
        if mark.emitted == emitted {
            // Nothing has been emitted since the snapshots were read out of the
            // model, so they still describe it. Deliberately *not* also gated on
            // `seen`: a late delivery of something already folded into the
            // snapshot does not make the snapshot wrong.
            //
            // This is the pan path, and the whole point of the mechanism.
            #[cfg(test)]
            {
                self.tally.reused += 1;
            }
            return SnapshotPlan::Reuse;
        }
        // Patch only when the dirty set provably covers the window
        // `(mark.emitted, emitted]`: nothing was queued at either end, and this
        // view was delivered exactly as many changes across it as the scene
        // emitted. Anything else means a change reached the model that this
        // view never saw, and only the model knows what it was.
        let emitted_since = emitted.wrapping_sub(mark.emitted);
        let delivered_since = self.seen.wrapping_sub(mark.seen);
        if !self.rebuild_all && mark.settled && settled && delivered_since == emitted_since {
            let ids: Vec<ItemId> = self.dirty.drain().collect();
            self.refreshed_at = Some(RefreshMark {
                emitted,
                seen: self.seen,
                settled,
            });
            #[cfg(test)]
            {
                self.tally.patched += 1;
            }
            return SnapshotPlan::Patch(ids);
        }
        self.reset_to(emitted, settled);
        SnapshotPlan::Rebuild
    }

    fn reset_to(&mut self, emitted: u64, settled: bool) {
        #[cfg(test)]
        {
            self.tally.rebuilt += 1;
        }
        self.dirty.clear();
        self.rebuild_all = false;
        self.refreshed_at = Some(RefreshMark {
            emitted,
            seen: self.seen,
            settled,
        });
    }
}

/// One item's hit geometry, read once and written into both snapshots.
///
/// The fields a [`HitInvalidation::Geometry`] change can move, and no others:
/// `key`, `claims_press`, `handlers` and `ignores_xform` are all invariant
/// under the patch path by construction (anything that could move them
/// classifies as [`HitInvalidation::Structure`]).
struct HitGeometry {
    scene_rect: Rect,
    scene_transform: Transform2D,
    shape: ItemShape,
    local_bounds: Rect,
    scene_anchor: Point,
}

impl HitGeometry {
    /// Read `id`'s geometry out of the model. `None` for an id with no
    /// lightweight item — a heavyweight entry, or one already removed — which
    /// is in neither snapshot anyway.
    fn read(scene: &Scene, id: ItemId, ignores_xform: bool) -> Option<Self> {
        let item = scene.item(id)?;
        let scene_rect = scene.scene_rect(id)?;
        let scene_transform = scene.scene_transform(id);
        Some(Self {
            scene_rect,
            scene_transform,
            shape: item.shape(),
            local_bounds: scene.local_bounds(id).unwrap_or(Rect::ZERO),
            // For a screen-anchored item the dispatch path projects this
            // through the *live* view transform at event time, so what is
            // stored is the scene-space anchor and it moves only when the item
            // does. Zero for everything else, where it is unused.
            scene_anchor: if ignores_xform {
                scene_transform.apply_point(Point::ZERO)
            } else {
                Point::ZERO
            },
        })
    }
}

impl SceneView {
    /// Bring both hit-test snapshots up to date with the model, doing the least
    /// work that leaves them correct.
    ///
    /// Called once per layout pass. A pan — which emits no change — leaves here
    /// after one `u64` comparison.
    pub(super) fn refresh_hit_snapshots(&self) {
        let scene = self.model.0.borrow();
        let emitted = scene.item_change_version();
        let settled = !scene.has_pending_notifications();
        let plan = self.hit_sync.borrow_mut().plan(emitted, settled);
        match plan {
            SnapshotPlan::Reuse => return,
            SnapshotPlan::Patch(ids) => self.patch_hit_snapshots(&scene, &ids),
            SnapshotPlan::Rebuild => self.rebuild_hit_snapshots(&scene),
        }
        // The `Over`-band veto memo is keyed on this counter, so it is retired
        // by exactly the passes that changed a row — and survives the ones that
        // did not, which is every pan sample.
        self.snapshot_generation
            .set(self.snapshot_generation.get().wrapping_add(1));
        self.veto_memo.set(None);
    }

    /// Rewrite the rows for `ids` and their subtrees in place.
    ///
    /// Rows are located by [`PaintKey`] through a binary search rather than a
    /// side index: the key is what the snapshots are sorted by, it is unique
    /// (its tie-break is the item id), and the patch path cannot change it — so
    /// the search is exact and there is no second structure to keep in sync.
    fn patch_hit_snapshots(&self, scene: &Scene, ids: &[ItemId]) {
        if ids.is_empty() {
            return;
        }
        // A change names one item; a move carries the subtree hanging off it,
        // because every descendant composes the moved frame.
        let mut affected: Vec<ItemId> = Vec::with_capacity(ids.len());
        for id in ids {
            affected.push(*id);
            scene.collect_descendants(*id, &mut affected);
        }

        let mut handlers = self.handler_snapshot.borrow_mut();
        let mut draggable = self.lightweight_bounds_snapshot.borrow_mut();
        for id in affected {
            let Some(key) = scene.paint_key(id) else {
                continue;
            };
            let ignores_xform = scene
                .flags(id)
                .unwrap_or_default()
                .contains(crate::flags::ItemFlags::IGNORES_TRANSFORMATIONS);
            let Some(geom) = HitGeometry::read(scene, id, ignores_xform) else {
                continue;
            };
            // Descending order, so the probe's comparison is reversed.
            if let Ok(i) = handlers.binary_search_by(|e| key.cmp(&e.key)) {
                let row = &mut handlers[i];
                row.scene_rect = geom.scene_rect;
                row.scene_transform = geom.scene_transform;
                row.shape = geom.shape.clone();
                row.local_bounds = geom.local_bounds;
                row.scene_anchor = geom.scene_anchor;
            }
            if let Ok(i) = draggable.binary_search_by(|e| key.cmp(&e.key)) {
                let row = &mut draggable[i];
                row.scene_rect = geom.scene_rect;
                row.scene_transform = geom.scene_transform;
                row.shape = geom.shape;
                row.local_bounds = geom.local_bounds;
                row.scene_anchor = geom.scene_anchor;
            }
        }
    }

    /// Build both snapshots from the model.
    ///
    /// The draggable snapshot carries only items that opted into drag via
    /// `.draggable(true)` (or `is_draggable()` on a custom impl). Without that
    /// filter every visible `RectItem` would answer a drag and the scene would
    /// feel unstable.
    ///
    /// The handler snapshot carries **every** hit-testable entry, not only the
    /// ones with handlers: a handler-less item painted on top still occludes
    /// what is beneath it inside the lightweight tier (the hit test resolves the
    /// topmost *entry* and only then looks for handlers), and an item without
    /// handlers can still carry a per-item cursor. `claims_press` — a different
    /// and narrower question — is recorded per row and read by exactly one
    /// thing, the `Over`-band veto.
    fn rebuild_hit_snapshots(&self, scene: &Scene) {
        let mut handlers = self.handler_snapshot.borrow_mut();
        let mut draggable = self.lightweight_bounds_snapshot.borrow_mut();
        handlers.clear();
        draggable.clear();
        let mut has_over_claimant = false;

        for entry in scene.entries.iter() {
            let id = entry.id;
            // Lightweight only: a heavyweight widget entry is hit-tested by the
            // arena's own walk, not from here.
            if scene.item(id).is_none() {
                continue;
            }
            // `IS_VISIBLE` says an item is "neither painted nor hit-tested";
            // `IS_ENABLED` says a disabled one "passes clicks through". Both
            // apply to a tap and to a grab, from one predicate.
            if !scene.is_hit_testable(id) {
                continue;
            }
            let Some(key) = scene.paint_key(id) else {
                continue;
            };
            let flags = entry.flags;
            let ignores_xform = flags.contains(crate::flags::ItemFlags::IGNORES_TRANSFORMATIONS);
            let Some(geom) = HitGeometry::read(scene, id, ignores_xform) else {
                continue;
            };

            let claims_press = crate::pick::claims_press(scene.handlers(id), flags);
            // Everything that can be painted **above a card**: the `Over`
            // band, which is above every card, and the `Interleaved` band,
            // which shares the cards' rank and is above the ones with a lower
            // `z`. `>=` rather than `==`: the gate is a cheap "is there
            // anything worth asking about at all" — the per-child key
            // comparison in `press_claimant_above` decides which cards it
            // actually vetoes.
            has_over_claimant |= claims_press && key.rank() >= crate::pick::RANK_WIDGET;

            if flags.contains(crate::flags::ItemFlags::IS_DRAGGABLE) {
                draggable.push(super::DraggableSnapshotEntry {
                    id,
                    scene_rect: geom.scene_rect,
                    scene_transform: geom.scene_transform,
                    shape: geom.shape.clone(),
                    ignores_xform,
                    scene_anchor: geom.scene_anchor,
                    local_bounds: geom.local_bounds,
                    key,
                });
            }
            handlers.push(HandlerSnapshotEntry {
                id,
                scene_rect: geom.scene_rect,
                scene_transform: geom.scene_transform,
                shape: geom.shape,
                key,
                claims_press,
                // Cheap: the set stores its closures as `Rc<dyn Fn>`, so this
                // is a refcount fan-out plus one box, and only for the items
                // that carry one at all.
                handlers: scene.handlers(id).cloned().map(Box::new),
                ignores_xform,
                scene_anchor: geom.scene_anchor,
                local_bounds: geom.local_bounds,
            });
        }

        // Topmost-first in the view's ONE paint order, so the first shape match
        // is the entry the user sees on top. A *stable descending* sort on `z`
        // alone — which this used to be — leaves equal-z ties in the ascending
        // order the entry list produced, so the first match was the oldest,
        // i.e. the bottom-most item: paint and hit were inverted on ties.
        // Comparing whole `PaintKey`s cannot have that failure mode.
        handlers.sort_by_key(|e| std::cmp::Reverse(e.key));
        draggable.sort_by_key(|e| std::cmp::Reverse(e.key));

        // The veto gate is refreshed with the snapshot it reads, so it never
        // outlives it.
        self.over_claimants.set(has_over_claimant);
    }
}

#[cfg(test)]
mod tests {
    //! The guards that make the cache safe, driven directly rather than through
    //! a mounted view — because the states they defend against (a queue left
    //! undrained by an observer panic, a change emitted before this view's
    //! observer existed) are states a healthy scene does not reach on demand.

    use super::*;
    use crate::item::ItemId;
    use teksilo_canvas::Point;

    fn moved(id: ItemId) -> ItemChange {
        ItemChange::LocalPosChanged {
            id,
            old: Point::ZERO,
            new: Point::new(1.0, 0.0),
        }
    }

    fn plan_kind(plan: SnapshotPlan) -> &'static str {
        match plan {
            SnapshotPlan::Reuse => "reuse",
            SnapshotPlan::Patch(_) => "patch",
            SnapshotPlan::Rebuild => "rebuild",
        }
    }

    /// The happy path, and the shape every other case is measured against: one
    /// emission, one delivery, a drained queue at both ends.
    #[test]
    fn a_seen_change_on_a_drained_queue_patches() {
        let mut sync = HitSnapshotSync::default();
        assert_eq!(plan_kind(sync.plan(0, true)), "rebuild", "first refresh");
        assert_eq!(plan_kind(sync.plan(0, true)), "reuse", "nothing emitted");
        sync.record(&moved(ItemId::next()));
        assert_eq!(plan_kind(sync.plan(1, true)), "patch");
    }

    /// The counters carry a permanent offset — the whole initial population was
    /// emitted before this view's observer existed — so the test has to be on
    /// the *delta*, and this pins that it is.
    #[test]
    fn a_permanent_offset_from_the_initial_population_still_patches() {
        let mut sync = HitSnapshotSync::default();
        // 500 items added before the observer: emitted 500, seen 0.
        assert_eq!(plan_kind(sync.plan(500, true)), "rebuild");
        sync.record(&moved(ItemId::next()));
        assert_eq!(
            plan_kind(sync.plan(501, true)),
            "patch",
            "comparing the counters absolutely would rebuild here forever",
        );
    }

    /// A change emitted but not delivered: the model moved and this view was not
    /// told which item. The dirty set is empty and would patch *nothing*.
    #[test]
    fn an_undelivered_change_rebuilds() {
        let mut sync = HitSnapshotSync::default();
        assert_eq!(plan_kind(sync.plan(0, true)), "rebuild");
        // Emitted two, delivered one — the second is still queued.
        sync.record(&moved(ItemId::next()));
        assert_eq!(plan_kind(sync.plan(2, false)), "rebuild");
    }

    /// A queue that was still draining at the *previous* refresh: the counts can
    /// balance across the window while covering the wrong changes, because the
    /// deliveries inside it are the tail of an older batch.
    #[test]
    fn a_window_that_opened_on_an_undrained_queue_rebuilds() {
        let mut sync = HitSnapshotSync::default();
        // First refresh happens with one change still queued.
        assert_eq!(plan_kind(sync.plan(1, false)), "rebuild");
        // That queued change is delivered, and one more is emitted and queued:
        // one emission, one delivery, and the queue looks drained now.
        sync.record(&moved(ItemId::next()));
        assert_eq!(
            plan_kind(sync.plan(2, true)),
            "rebuild",
            "the delivery inside this window was the *previous* window's change",
        );
    }

    /// A structural change poisons the window even when delivery is perfect.
    #[test]
    fn a_structural_change_rebuilds() {
        let mut sync = HitSnapshotSync::default();
        assert_eq!(plan_kind(sync.plan(0, true)), "rebuild");
        sync.record(&moved(ItemId::next()));
        sync.record(&ItemChange::ZChanged {
            id: ItemId::next(),
            old: 0.0,
            new: 1.0,
        });
        assert_eq!(plan_kind(sync.plan(2, true)), "rebuild");
    }

    /// A change the snapshots hold nothing for still counts as a delivery: it
    /// advanced the model's emission counter, so leaving it out of `seen` would
    /// make the next window look under-delivered and rebuild forever.
    #[test]
    fn an_irrelevant_change_is_still_counted() {
        let mut sync = HitSnapshotSync::default();
        assert_eq!(plan_kind(sync.plan(0, true)), "rebuild");
        sync.record(&ItemChange::OpacityChanged {
            id: ItemId::next(),
            old: 1.0,
            new: 0.5,
        });
        assert_eq!(
            plan_kind(sync.plan(1, true)),
            "patch",
            "an opacity change patches an empty set, which is the cheapest \
             correct answer — not a rebuild, and not a stale reuse",
        );
    }

    /// The dirty set must not grow without bound across a run of structural
    /// changes that will be rebuilt from the model anyway.
    #[test]
    fn a_structural_change_drops_the_pending_dirty_set() {
        let mut sync = HitSnapshotSync::default();
        sync.record(&moved(ItemId::next()));
        sync.record(&ItemChange::Removed { id: ItemId::next() });
        assert!(sync.dirty.is_empty());
    }
}
