// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Binding registry: the dirty-tracking infrastructure shared between
//! `Signal<T>` instances and `WidgetTree`.
//!
//! Despite the module name (kept for historical reasons), this file no
//! longer contains any state primitive — `Signal<T>` in `signal.rs` is
//! the only reactive type. The registry, its binding entries, and the
//! `BindingLevel` enum live here because they are the shared vocabulary
//! between signals and the widget tree.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::widget_id::WidgetId;

/// Dirty-tracking granularity for a property binding.
/// Determined by the primitive widget implementor, not the consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingLevel {
    /// Visual-only change (color, opacity). Marks the widget for repaint;
    /// layout is skipped.
    RepaintOnly,
    /// Visual-only change that propagates through the entire subtree.
    /// Used by `enabled_when` so that flipping a single node's
    /// `enabled_state` marks the whole disabled subtree for repaint —
    /// leaves like `IconWidget` resolve their role color from
    /// [`crate::widget::PaintContext::effective_enabled`], which the
    /// paint walker computes by AND-ing ancestor enabled-states.
    /// Without this propagation, only the bound node would repaint and
    /// descendants would keep their stale enabled colors.
    SubtreeRepaint,
    /// Size-affecting change (text content, constraint value). Marks the widget
    /// for relayout and propagates upward through ancestors.
    Relayout,
    /// Data-model change requiring the widget's `build()` to re-run.
    /// Used by data-driven widgets (Repeater, ListView, TreeView) to trigger
    /// a full rebuild of their child subtree when the underlying data changes.
    Rebuild,
    /// Accessibility-tree change only. Flips `WidgetTree::a11y_dirty` without
    /// touching repaint, layout, or rebuild flags. Orthogonal to the other
    /// levels — widgets whose accessibility output depends on a signal that
    /// does not visually affect the widget itself (e.g., a `document_version`
    /// bumped by every text edit) bind at this level so AccessKit's tree
    /// rebuilds as soon as the underlying data changes, regardless of
    /// whether the widget needs repainting.
    AccessibilityOnly,
}

/// A registered binding between a signal and a widget property.
///
/// Carries the source-signal generation closure so the registry can
/// construct a [`BindingGroup`] on first registration. After that, all
/// bindings sharing one `source_id` collapse into a single group entry —
/// the closure is stored once on the group and never duplicated.
/// Pre-optimization the registry walked all bindings every frame and
/// polled per binding, even though many shared the same underlying
/// source.
#[derive(Clone)]
pub(crate) struct Binding {
    /// Widget to mark dirty when the source signal changes.
    pub widget_id: WidgetId,
    /// The dirty-tracking level for this binding.
    pub level: BindingLevel,
    /// Read the source signal's current change generation.
    pub generation: Rc<dyn Fn() -> u64>,
    /// Stable identity of the source signal — see
    /// `Signal::source_id`.
    /// Used by [`BindingRegistry::register`] to look up the matching
    /// [`BindingGroup`] in O(1).
    pub source_id: usize,
}

/// All bindings that share one source signal.
///
/// Stored once per `source_id` in the registry's `HashMap`. The
/// generation closure is captured at first-registration time and reused
/// for every subsequent binding on the same source — N bindings on one
/// signal cost one poll per frame, not N.
struct BindingGroup {
    generation: Rc<dyn Fn() -> u64>,
    /// The source generation this registry last acted on. **This is the
    /// consumer half of dirty tracking, and it lives here on purpose.**
    ///
    /// Each `WidgetTree` owns its own `BindingRegistry`, so N open
    /// windows bound to one shared `Signal` get N independent
    /// `last_seen` values. Keeping it on the signal instead — as the
    /// `dirty: bool` this replaced did — meant one slot for N consumers,
    /// and a flush that both read and cleared it: whichever tree
    /// reconciled first consumed the flag and every other window
    /// silently skipped its binding, permanently. See
    /// `signal::MutableInner::generation`.
    ///
    /// A `Cell` rather than a plain field so [`flush_all_dirty`] can
    /// update it while holding only a shared borrow of the map.
    last_seen: Cell<u64>,
    /// `(widget_id, level)` pairs to flush when this source is dirty.
    /// `AccessibilityOnly` lives in the same Vec as visual levels —
    /// the flush walker dispatches on `level` to the right bucket.
    /// Dedupe key within the Vec is `(widget_id, is_a11y(level))`,
    /// preserving the original "a11y vs visual buckets are separate"
    /// semantics so a widget can hold one of each on the same source.
    bindings: Vec<(WidgetId, BindingLevel)>,
}

/// Shared registry of all active property bindings.
///
/// Indexed by source-signal identity (`source_id`) so the per-frame
/// flush iterates unique sources rather than every binding. Typical
/// catalog scene: ~30-40 unique sources covering 100-300+ bindings
/// (every reactive theme query, every `label`, every visibility
/// signal pulls a binding off the same shared root).
///
/// **One registry per [`WidgetTree`](crate::WidgetTree)**, and that
/// matters: the registry is where "have I acted on this change yet?" is
/// remembered, so two windows sharing a `Signal` each answer it for
/// themselves. Cloning a `BindingRegistry` shares the same state (it is
/// `Rc`-backed) and is only ever done to hold onto one tree's registry,
/// never to hand a second tree the first tree's bookkeeping.
#[derive(Clone, Default)]
pub struct BindingRegistry {
    by_source: Rc<RefCell<HashMap<usize, BindingGroup>>>,
}

impl BindingRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn register(&self, binding: Binding) {
        let mut by_source = self.by_source.borrow_mut();
        let group = by_source.entry(binding.source_id).or_insert_with(|| {
            // Seed `last_seen` at the source's CURRENT generation: the
            // widget registering this binding has, by construction, just
            // read the current value in its `build()`, so it is not stale
            // and must not fire on the very next flush.
            //
            // Only on group *creation*. A later binding joining an
            // existing group inherits that group's `last_seen`, which may
            // be older — deliberately: one widget rebuilding must not
            // swallow a pending change on behalf of the other widgets
            // bound to the same source. Inheriting an older value can at
            // worst cost the newcomer one redundant repaint.
            let generation = binding.generation.clone();
            let seen = generation();
            BindingGroup {
                generation,
                last_seen: Cell::new(seen),
                bindings: Vec::new(),
            }
        });
        // Dedup within the group by (widget_id, a11y bucket). Visual
        // levels collapse with each other (and promote); a11y stays
        // in its own bucket so a widget can hold both flavours on
        // one source without one clobbering the other.
        let incoming_a11y = is_a11y_only(binding.level);
        if let Some(existing) = group
            .bindings
            .iter_mut()
            .find(|(wid, lvl)| *wid == binding.widget_id && is_a11y_only(*lvl) == incoming_a11y)
        {
            existing.1 = promote_level(existing.1, binding.level);
            return;
        }
        group.bindings.push((binding.widget_id, binding.level));
    }

    /// Drop every binding targeting `widget_id`. Called by the widget
    /// tree before a widget rebuilds (so `build()` can re-register a
    /// fresh, deduplicated set) and on destroy (so a dead widget's
    /// bindings no longer keep source-signal references alive or
    /// accumulate across the lifetime of the app).
    ///
    /// A group left with no bindings is **kept** here and reclaimed
    /// later by [`reclaim_empty_groups`](Self::reclaim_empty_groups),
    /// which the tree calls once at the end of each reconcile pass.
    /// Dropping it immediately would throw away the group's
    /// `last_seen`, and a rebuild is exactly unregister-then-register:
    /// a widget that is the only binder of a source would come out of
    /// its own rebuild with `last_seen` re-seeded at *now*, silently
    /// swallowing any write made in between — and `build()` commonly
    /// writes before it re-binds (`SceneView` bumps `reconcile_dirty`
    /// from the item-change observer its dynamic-bounds refresh fires,
    /// several lines before re-registering it). Deferring reclamation
    /// to end-of-pass keeps the ledger across a rebuild while still
    /// reclaiming it for a widget that was genuinely destroyed.
    pub(crate) fn unregister_for_widget(&self, widget_id: WidgetId) {
        let mut by_source = self.by_source.borrow_mut();
        for group in by_source.values_mut() {
            group.bindings.retain(|(wid, _)| *wid != widget_id);
        }
    }

    /// Drop every group that no longer has any bindings, reclaiming
    /// its `source_id` slot and releasing its reference to the source
    /// signal. Called once per reconcile pass, after rebuilds have had
    /// their chance to re-register — see
    /// [`unregister_for_widget`](Self::unregister_for_widget) for why
    /// reclamation is deferred rather than immediate.
    pub(crate) fn reclaim_empty_groups(&self) {
        self.by_source
            .borrow_mut()
            .retain(|_src, group| !group.bindings.is_empty());
    }

    /// Number of live bindings. Exposed for tests that verify
    /// cleanup does not accumulate entries across rebuilds. Equals
    /// the total count of `(widget_id, level)` entries across all
    /// source groups.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.by_source
            .borrow()
            .values()
            .map(|g| g.bindings.len())
            .sum()
    }

    /// Drain dirty bindings in one pass — return both the visual
    /// dirty list (per-widget at the highest visual level seen) and
    /// the tree-wide accessibility-dirty flag.
    ///
    /// "Dirty" is `source generation != the generation this registry
    /// last acted on`, and acting on it advances only THIS registry's
    /// `last_seen` — nothing on the signal is mutated. Two consequences
    /// worth stating, because the previous shared-`bool` design got both
    /// wrong:
    ///
    /// - A signal bound at both `AccessibilityOnly` and some visual
    ///   level (e.g. `Button::label` registers at `RepaintOnly` for the
    ///   inner TextWidget AND at `AccessibilityOnly` for the AT name)
    ///   lands in one group with one `last_seen`, so both buckets see
    ///   the same generation in the same pass. There is no longer an
    ///   ordering hazard to design around: nothing is consumed, so no
    ///   bucket can starve another.
    /// - Another `WidgetTree`'s registry flushing the same shared signal
    ///   has no effect here at all. That is the whole point — see
    ///   [`BindingGroup::last_seen`].
    ///
    /// Cost is O(S) generation polls (S = unique sources) plus O(D)
    /// widget-level promotions (D = bindings on dirty sources).
    /// Pre-optimization it was O(N) polls (N = all bindings); on the
    /// catalog scene S≈30-40, N≈100-300.
    pub(crate) fn flush_all_dirty(&self) -> (Vec<(WidgetId, BindingLevel)>, bool) {
        let by_source = self.by_source.borrow();
        let mut dirty_map: HashMap<WidgetId, BindingLevel> = HashMap::new();
        let mut a11y_dirty = false;
        for group in by_source.values() {
            let generation = (group.generation)();
            if generation == group.last_seen.get() {
                continue;
            }
            group.last_seen.set(generation);
            for &(wid, level) in &group.bindings {
                match level {
                    BindingLevel::AccessibilityOnly => {
                        a11y_dirty = true;
                    }
                    BindingLevel::RepaintOnly
                    | BindingLevel::SubtreeRepaint
                    | BindingLevel::Relayout
                    | BindingLevel::Rebuild => {
                        let entry = dirty_map.entry(wid).or_insert(level);
                        *entry = promote_level(*entry, level);
                    }
                }
            }
        }
        (dirty_map.into_iter().collect(), a11y_dirty)
    }

    /// Whether any bound source has advanced past what this registry
    /// last acted on — i.e. whether a [`flush_all_dirty`](Self::flush_all_dirty)
    /// right now would return anything.
    ///
    /// O(S) `u64` comparisons over unique sources, with no arena walk,
    /// no rebuilds and no layout. Read-only: unlike the flush it does
    /// NOT advance `last_seen`, so asking is free of consequence and can
    /// be repeated.
    ///
    /// Exists because a poll-based design can otherwise only answer
    /// "does this tree have pending reactive work?" by running the whole
    /// reconcile pass. Cross-window schedulers care about that question
    /// — see `bastyde_app::WindowManager::request_redraw_needing_render`.
    pub fn any_dirty(&self) -> bool {
        self.by_source
            .borrow()
            .values()
            .any(|group| (group.generation)() != group.last_seen.get())
    }

    /// Visual-only flush. Wrapper around [`flush_all_dirty`] that
    /// discards the accessibility flag — kept for tests that drive
    /// the registry directly. Production code (the widget tree's
    /// `process_state_changes`) calls `flush_all_dirty` so both
    /// buckets stay coherent in one pass.
    #[cfg(test)]
    pub(crate) fn flush_dirty(&self) -> Vec<(WidgetId, BindingLevel)> {
        self.flush_all_dirty().0
    }

    /// Accessibility-only flush. Wrapper around [`flush_all_dirty`].
    /// Same caveat as [`flush_dirty`]: production code should call
    /// `flush_all_dirty` once instead of pairing this with
    /// `flush_dirty`, since both share one walk and one clear pass.
    #[cfg(test)]
    pub(crate) fn flush_accessibility_dirty(&self) -> bool {
        self.flush_all_dirty().1
    }
}

/// Priority order for visual binding levels — `Rebuild` dominates
/// `Relayout` dominates `SubtreeRepaint` dominates `RepaintOnly`.
/// `AccessibilityOnly` lives in its own bucket and is never compared
/// against visual levels.
///
/// `SubtreeRepaint` is treated as strictly more work than
/// `RepaintOnly` because it covers a wider area (one node vs. a whole
/// subtree); when both happen on the same node `SubtreeRepaint` wins.
fn promote_level(existing: BindingLevel, incoming: BindingLevel) -> BindingLevel {
    use BindingLevel::*;
    match (existing, incoming) {
        (Rebuild, _) | (_, Rebuild) => Rebuild,
        (Relayout, _) | (_, Relayout) => Relayout,
        (SubtreeRepaint, _) | (_, SubtreeRepaint) => SubtreeRepaint,
        (RepaintOnly, _) | (_, RepaintOnly) => RepaintOnly,
        (AccessibilityOnly, AccessibilityOnly) => AccessibilityOnly,
    }
}

fn is_a11y_only(level: BindingLevel) -> bool {
    matches!(level, BindingLevel::AccessibilityOnly)
}

impl std::fmt::Debug for BindingRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let by_source = self.by_source.borrow();
        let total: usize = by_source.values().map(|g| g.bindings.len()).sum();
        f.debug_struct("BindingRegistry")
            .field("sources", &by_source.len())
            .field("bindings", &total)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::Signal;
    use std::cell::Cell;
    use std::rc::Rc;

    /// A binding over a hand-driven generation counter, standing in for
    /// a real signal's. Bump the counter to simulate a write.
    fn make_binding(level: BindingLevel, generation: Rc<Cell<u64>>) -> Binding {
        let read = {
            let g = generation.clone();
            Rc::new(move || g.get()) as Rc<dyn Fn() -> u64>
        };
        // WidgetId value doesn't matter for these tests; slotmap
        // default gives us a well-formed id.
        let id: WidgetId = slotmap::KeyData::from_ffi(1).into();
        Binding {
            widget_id: id,
            level,
            generation: read,
            // Bindings sharing one counter share one source id, so
            // passing the same `Rc` twice exercises the group path and
            // passing a fresh one exercises distinct sources.
            source_id: Rc::as_ptr(&generation) as *const () as usize,
        }
    }

    #[test]
    fn register_dedups_same_widget_same_signal_same_bucket() {
        use crate::signal::Signal;
        let reg = BindingRegistry::new();
        let sig = Signal::new(0_i32);
        let id: WidgetId = slotmap::KeyData::from_ffi(7).into();

        sig.bind_to(id, &reg, BindingLevel::RepaintOnly);
        sig.bind_to(id, &reg, BindingLevel::RepaintOnly);
        sig.bind_to(id, &reg, BindingLevel::RepaintOnly);

        assert_eq!(
            reg.len(),
            1,
            "three identical bind_to calls must collapse to one entry"
        );
    }

    #[test]
    fn register_promotes_level_on_dedup() {
        use crate::signal::Signal;
        let reg = BindingRegistry::new();
        let sig = Signal::new(0_i32);
        let id: WidgetId = slotmap::KeyData::from_ffi(7).into();

        sig.bind_to(id, &reg, BindingLevel::RepaintOnly);
        sig.bind_to(id, &reg, BindingLevel::Relayout);
        sig.bind_to(id, &reg, BindingLevel::RepaintOnly);

        assert_eq!(reg.len(), 1, "dedup still collapses across calls");
        // Signal must now be marked dirty so flush_dirty sees it.
        sig.set(1);
        let visual = reg.flush_dirty();
        assert_eq!(visual.len(), 1);
        assert_eq!(
            visual[0].1,
            BindingLevel::Relayout,
            "the merged entry reflects the highest-priority visual level seen"
        );
    }

    #[test]
    fn register_does_not_collapse_a11y_and_visual_buckets() {
        use crate::signal::Signal;
        let reg = BindingRegistry::new();
        let sig = Signal::new(0_i32);
        let id: WidgetId = slotmap::KeyData::from_ffi(7).into();

        sig.bind_to(id, &reg, BindingLevel::RepaintOnly);
        sig.bind_to(id, &reg, BindingLevel::AccessibilityOnly);

        assert_eq!(
            reg.len(),
            2,
            "visual and a11y-only bindings live in distinct buckets"
        );
    }

    #[test]
    fn register_distinct_signals_remain_distinct() {
        use crate::signal::Signal;
        let reg = BindingRegistry::new();
        let a = Signal::new(0_i32);
        let b = Signal::new(0_i32);
        let id: WidgetId = slotmap::KeyData::from_ffi(7).into();

        a.bind_to(id, &reg, BindingLevel::Relayout);
        b.bind_to(id, &reg, BindingLevel::Relayout);

        assert_eq!(
            reg.len(),
            2,
            "different signals must not collapse into one binding"
        );
    }

    #[test]
    fn flush_dirty_excludes_accessibility_only_from_visual_map() {
        // AccessibilityOnly bindings flow through the a11y bucket of
        // `flush_all_dirty`, never the visual map.
        let reg = BindingRegistry::new();
        let generation = Rc::new(Cell::new(0_u64));
        reg.register(make_binding(
            BindingLevel::AccessibilityOnly,
            generation.clone(),
        ));
        generation.set(1);

        let (visual, a11y_dirty) = reg.flush_all_dirty();
        assert!(
            visual.is_empty(),
            "AccessibilityOnly must not appear in the visual dirty map"
        );
        assert!(a11y_dirty, "AccessibilityOnly binding must set a11y flag");
        assert!(
            !reg.any_dirty(),
            "the flush advanced this registry's last-seen generation"
        );
    }

    #[test]
    fn flush_accessibility_dirty_returns_true_then_settles() {
        let reg = BindingRegistry::new();
        let generation = Rc::new(Cell::new(0_u64));
        reg.register(make_binding(
            BindingLevel::AccessibilityOnly,
            generation.clone(),
        ));
        generation.set(1);

        assert!(reg.flush_accessibility_dirty());
        assert_eq!(
            generation.get(),
            1,
            "the source generation is never reset by a flush — only the \
             registry's own last-seen advances"
        );
        assert!(
            !reg.flush_accessibility_dirty(),
            "second drain returns false (nothing new)"
        );
    }

    #[test]
    fn flush_all_dirty_drains_visual_and_a11y_in_one_pass() {
        // A Signal bound at both RepaintOnly and AccessibilityOnly
        // shares one source group. A single `flush_all_dirty` call must
        // surface both sides — under the old read-and-clear design this
        // needed a deliberate "collect everything before clearing"
        // dance; with generations there is nothing to consume.
        let reg = BindingRegistry::new();
        let shared = Rc::new(Cell::new(0_u64));
        reg.register(make_binding(BindingLevel::RepaintOnly, shared.clone()));
        reg.register(make_binding(
            BindingLevel::AccessibilityOnly,
            shared.clone(),
        ));
        shared.set(1);

        let (visual, a11y_dirty) = reg.flush_all_dirty();
        assert_eq!(visual.len(), 1, "visual binding must fire");
        assert!(a11y_dirty, "a11y binding must fire from the same source");

        let (visual, a11y_dirty) = reg.flush_all_dirty();
        assert!(visual.is_empty() && !a11y_dirty, "second pass is clean");
    }

    // ─── Per-consumer dirty tracking (the cross-window fix) ──────────

    #[test]
    fn two_registries_on_one_source_each_see_the_change() {
        // THE regression. Two independent `WidgetTree`s (each owning
        // its own registry) bound to ONE shared Signal: a single write
        // must be visible to BOTH, in either flush order. The previous
        // `dirty: bool` lived on the signal and was cleared by whoever
        // flushed first, so the second window silently — and
        // permanently — skipped its binding.
        let a = BindingRegistry::new();
        let b = BindingRegistry::new();
        let sig = Signal::new(0_i32);
        let id: WidgetId = slotmap::KeyData::from_ffi(3).into();
        sig.bind_to(id, &a, BindingLevel::Relayout);
        sig.bind_to(id, &b, BindingLevel::Relayout);

        sig.set(1);

        assert_eq!(a.flush_dirty().len(), 1, "first registry to flush fires");
        assert_eq!(
            b.flush_dirty().len(),
            1,
            "and the second one fires too — the first flush consumed nothing"
        );
        assert!(
            a.flush_dirty().is_empty(),
            "neither re-fires without a write"
        );
        assert!(b.flush_dirty().is_empty());
    }

    #[test]
    fn many_registries_on_one_source_all_see_every_change() {
        // Same property at N > 2, and across successive writes, so a
        // fix that merely swapped which single consumer wins would fail
        // here.
        let regs: Vec<BindingRegistry> = (0..4).map(|_| BindingRegistry::new()).collect();
        let sig = Signal::new(0_i32);
        let id: WidgetId = slotmap::KeyData::from_ffi(4).into();
        for reg in &regs {
            sig.bind_to(id, reg, BindingLevel::RepaintOnly);
        }

        for round in 1..=3 {
            sig.set(round);
            for (i, reg) in regs.iter().enumerate() {
                assert_eq!(
                    reg.flush_dirty().len(),
                    1,
                    "registry {i} missed the write in round {round}"
                );
            }
        }
    }

    #[test]
    fn a_registry_that_never_flushes_does_not_starve_the_others() {
        // The asymmetric case: one window is minimised / never
        // reconciles for many writes. Its backlog must neither block
        // the others nor accumulate into more than one fire when it
        // finally does flush — a generation compare collapses N missed
        // writes into "you are behind", which is exactly right for a
        // repaint.
        let live = BindingRegistry::new();
        let asleep = BindingRegistry::new();
        let sig = Signal::new(0_i32);
        let id: WidgetId = slotmap::KeyData::from_ffi(5).into();
        sig.bind_to(id, &live, BindingLevel::RepaintOnly);
        sig.bind_to(id, &asleep, BindingLevel::RepaintOnly);

        for round in 1..=5 {
            sig.set(round);
            assert_eq!(live.flush_dirty().len(), 1, "live registry keeps up");
        }

        assert!(asleep.any_dirty(), "the sleeper is behind, and knows it");
        assert_eq!(
            asleep.flush_dirty().len(),
            1,
            "it catches up in one fire, not five"
        );
        assert!(!asleep.any_dirty());
    }

    #[test]
    fn a_binding_registered_after_a_write_is_not_retroactively_dirty() {
        // A widget that binds during `build()` has just read the current
        // value, so it must not fire on the next flush for a write that
        // predates it.
        let reg = BindingRegistry::new();
        let sig = Signal::new(0_i32);
        let id: WidgetId = slotmap::KeyData::from_ffi(6).into();

        sig.set(1);
        sig.bind_to(id, &reg, BindingLevel::Relayout);

        assert!(
            reg.flush_dirty().is_empty(),
            "registration seeds last-seen at the current generation"
        );
        sig.set(2);
        assert_eq!(reg.flush_dirty().len(), 1, "but the NEXT write does fire");
    }

    #[test]
    fn joining_an_existing_group_does_not_swallow_its_pending_change() {
        // One widget rebuilding (and re-registering) must not seed a
        // fresh last-seen for the whole group — the OTHER widgets bound
        // to that source have not reconciled yet.
        let reg = BindingRegistry::new();
        let sig = Signal::new(0_i32);
        let first: WidgetId = slotmap::KeyData::from_ffi(8).into();
        let second: WidgetId = slotmap::KeyData::from_ffi(9).into();
        sig.bind_to(first, &reg, BindingLevel::RepaintOnly);

        sig.set(1);
        // A second widget binds the same source after the write.
        sig.bind_to(second, &reg, BindingLevel::RepaintOnly);

        let dirty = reg.flush_dirty();
        assert_eq!(
            dirty.len(),
            2,
            "the group keeps its older last-seen, so the widget that had \
             NOT yet reconciled still fires (the newcomer's extra repaint \
             is the accepted cost)"
        );
    }

    #[test]
    fn any_dirty_is_read_only() {
        let reg = BindingRegistry::new();
        let sig = Signal::new(0_i32);
        let id: WidgetId = slotmap::KeyData::from_ffi(10).into();
        sig.bind_to(id, &reg, BindingLevel::RepaintOnly);

        assert!(!reg.any_dirty(), "nothing written yet");
        sig.set(1);
        assert!(reg.any_dirty());
        assert!(reg.any_dirty(), "asking twice must not consume the answer");
        assert_eq!(reg.flush_dirty().len(), 1, "the flush still fires");
        assert!(!reg.any_dirty());
    }

    #[test]
    fn signal_bind_to_accessibility_only_propagates_via_registry() {
        // End-to-end: a real Signal<T> bound at AccessibilityOnly
        // fires the a11y flag from `flush_all_dirty` without
        // appearing in the visual dirty map.
        let reg = BindingRegistry::new();
        let sig = Signal::new(0_u64);
        let id: WidgetId = slotmap::KeyData::from_ffi(7).into();
        sig.bind_to(id, &reg, BindingLevel::AccessibilityOnly);

        // Fresh binding is not dirty yet.
        let (visual, a11y) = reg.flush_all_dirty();
        assert!(visual.is_empty());
        assert!(!a11y);

        sig.set(1);
        let (visual, a11y) = reg.flush_all_dirty();
        assert!(visual.is_empty(), "a11y flips must not leak to visual");
        assert!(a11y);
        // Subsequent drain is clean again.
        assert!(!reg.flush_accessibility_dirty());
    }

    // ─── Source-indexed registry semantics ───────────────────────────

    #[test]
    fn phase4_register_dedup_same_source_same_widget() {
        // Identical (widget, source, bucket) triples collapse to a
        // single entry inside the source group — no double-fire.
        use crate::signal::Signal;
        let reg = BindingRegistry::new();
        let sig = Signal::new(0_i32);
        let id: WidgetId = slotmap::KeyData::from_ffi(11).into();

        sig.bind_to(id, &reg, BindingLevel::RepaintOnly);
        sig.bind_to(id, &reg, BindingLevel::RepaintOnly);
        sig.bind_to(id, &reg, BindingLevel::RepaintOnly);

        assert_eq!(reg.len(), 1, "three identical bind_to calls collapse");

        sig.set(1);
        let (visual, _a11y) = reg.flush_all_dirty();
        assert_eq!(
            visual.len(),
            1,
            "deduplicated binding must fire exactly once"
        );
    }

    #[test]
    fn phase4_one_widget_can_hold_visual_and_a11y_on_one_source() {
        // (widget_id, source_id, AccessibilityOnly) is a separate
        // bucket from (widget_id, source_id, visual). Both must coexist.
        use crate::signal::Signal;
        let reg = BindingRegistry::new();
        let sig = Signal::new(0_i32);
        let id: WidgetId = slotmap::KeyData::from_ffi(13).into();

        sig.bind_to(id, &reg, BindingLevel::RepaintOnly);
        sig.bind_to(id, &reg, BindingLevel::AccessibilityOnly);
        assert_eq!(reg.len(), 2, "different buckets stay distinct");

        sig.set(1);
        let (visual, a11y) = reg.flush_all_dirty();
        assert_eq!(visual.len(), 1);
        assert!(a11y);
    }

    #[test]
    fn phase4_one_signal_many_widgets_one_dirty_check() {
        // Source group folds multiple widget bindings under one
        // dirty closure — visible from the outside as: setting the
        // signal once dirties N widgets in one flush.
        use crate::signal::Signal;
        let reg = BindingRegistry::new();
        let sig = Signal::new(0_i32);
        let ids: Vec<WidgetId> = (0..5)
            .map(|i| slotmap::KeyData::from_ffi(20 + i).into())
            .collect();
        for id in &ids {
            sig.bind_to(*id, &reg, BindingLevel::Relayout);
        }
        assert_eq!(reg.len(), 5);

        sig.set(1);
        let (visual, _a11y) = reg.flush_all_dirty();
        assert_eq!(visual.len(), 5, "every binding on the source fires");
    }

    #[test]
    fn phase4_level_promotion_preserved() {
        // Re-registering at a higher level promotes; lower or equal
        // is a no-op. The priority order must be preserved
        // (Rebuild > Relayout > RepaintOnly).
        use crate::signal::Signal;
        let reg = BindingRegistry::new();
        let sig = Signal::new(0_i32);
        let id: WidgetId = slotmap::KeyData::from_ffi(33).into();

        sig.bind_to(id, &reg, BindingLevel::RepaintOnly);
        sig.bind_to(id, &reg, BindingLevel::Relayout);
        sig.bind_to(id, &reg, BindingLevel::RepaintOnly); // shouldn't demote

        sig.set(1);
        let (visual, _) = reg.flush_all_dirty();
        assert_eq!(visual.len(), 1);
        assert_eq!(visual[0].1, BindingLevel::Relayout);
    }

    #[test]
    fn phase4_unregister_for_widget_drops_empty_groups_at_end_of_pass() {
        // After unregistering the only widget on a source, the group
        // is reclaimed — but at the END of the reconcile pass, not
        // instantly, so a rebuild can re-register into it first (see
        // the next test). Source slots are still reused without memory
        // growth across rebuilds.
        use crate::signal::Signal;
        let reg = BindingRegistry::new();
        let sig = Signal::new(0_i32);
        let id: WidgetId = slotmap::KeyData::from_ffi(44).into();

        sig.bind_to(id, &reg, BindingLevel::RepaintOnly);
        assert_eq!(reg.by_source.borrow().len(), 1);

        reg.unregister_for_widget(id);
        assert_eq!(reg.len(), 0, "the binding itself is gone immediately");

        reg.reclaim_empty_groups();
        assert_eq!(
            reg.by_source.borrow().len(),
            0,
            "empty groups must be reclaimed by the end-of-pass sweep"
        );
    }

    #[test]
    fn a_rebuild_does_not_swallow_a_write_its_own_build_made() {
        // A widget's rebuild is unregister → `build()` → re-register,
        // and `build()` commonly writes a signal BEFORE re-binding it
        // (`SceneView` bumps `reconcile_dirty` from the item-change
        // observer that its dynamic-bounds refresh fires, well above
        // its own `bind_to` call). If dropping the emptied group also
        // dropped its `last_seen`, re-registration would re-seed at the
        // post-write generation and that write would vanish — the
        // widget's follow-up rebuild would simply never be armed.
        use crate::signal::Signal;
        let reg = BindingRegistry::new();
        let sig = Signal::new(0_i32);
        let id: WidgetId = slotmap::KeyData::from_ffi(45).into();
        sig.bind_to(id, &reg, BindingLevel::Rebuild);
        // The pass that decided to rebuild already drained the flush.
        sig.set(1);
        assert_eq!(reg.flush_dirty().len(), 1);

        // ── the rebuild ──
        reg.unregister_for_widget(id);
        sig.set(2); // `build()` writes...
        sig.bind_to(id, &reg, BindingLevel::Rebuild); // ...then re-binds
        reg.reclaim_empty_groups(); // end of pass

        assert_eq!(
            reg.flush_dirty().len(),
            1,
            "the write made during build() must still arm the next rebuild"
        );
    }

    #[test]
    fn a_destroyed_widgets_group_is_still_reclaimed() {
        // The other side of deferring: a widget that unregisters and
        // does NOT come back must not leave its group (and its strong
        // reference to the source signal) behind.
        use crate::signal::Signal;
        let reg = BindingRegistry::new();
        let sig = Signal::new(0_i32);
        let gone: WidgetId = slotmap::KeyData::from_ffi(46).into();
        let stays: WidgetId = slotmap::KeyData::from_ffi(47).into();
        sig.bind_to(gone, &reg, BindingLevel::RepaintOnly);
        sig.bind_to(stays, &reg, BindingLevel::RepaintOnly);

        reg.unregister_for_widget(gone);
        reg.reclaim_empty_groups();
        assert_eq!(
            reg.by_source.borrow().len(),
            1,
            "a group with a surviving binding is kept"
        );

        reg.unregister_for_widget(stays);
        reg.reclaim_empty_groups();
        assert_eq!(reg.by_source.borrow().len(), 0, "now it is reclaimed");
    }
}
