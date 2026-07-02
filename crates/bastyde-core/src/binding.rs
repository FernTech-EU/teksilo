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

use std::cell::RefCell;
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
/// Carries the source-signal closures so the registry can construct a
/// [`BindingGroup`] on first registration. After that, all bindings
/// sharing one `source_id` collapse into a single group entry — the
/// closures are stored once on the group and never duplicated.
/// Pre-optimization the registry walked all bindings every frame and
/// called `is_dirty()` per binding, even though many shared the same
/// underlying dirty flag.
#[derive(Clone)]
pub(crate) struct Binding {
    /// Widget to mark dirty when the source signal changes.
    pub widget_id: WidgetId,
    /// The dirty-tracking level for this binding.
    pub level: BindingLevel,
    /// Check if the source signal is dirty.
    pub is_dirty: Rc<dyn Fn() -> bool>,
    /// Clear the dirty flag on the source signal.
    pub clear_dirty: Rc<dyn Fn()>,
    /// Stable identity of the source signal — see
    /// `Signal::source_id`.
    /// Used by [`BindingRegistry::register`] to look up the matching
    /// [`BindingGroup`] in O(1).
    pub source_id: usize,
}

/// All bindings that share one source signal.
///
/// Stored once per `source_id` in the registry's `HashMap`. The dirty
/// closures are captured at first-registration time and reused for
/// every subsequent binding on the same source — N bindings on one
/// signal cost one `is_dirty()` call per frame, not N.
struct BindingGroup {
    is_dirty: Rc<dyn Fn() -> bool>,
    clear_dirty: Rc<dyn Fn()>,
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
        let group = by_source
            .entry(binding.source_id)
            .or_insert_with(|| BindingGroup {
                is_dirty: binding.is_dirty.clone(),
                clear_dirty: binding.clear_dirty.clone(),
                bindings: Vec::new(),
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
    pub(crate) fn unregister_for_widget(&self, widget_id: WidgetId) {
        let mut by_source = self.by_source.borrow_mut();
        // Two-pass — drop the widget's entries from each group, then
        // drop empty groups so source_id slots can be reclaimed.
        by_source.retain(|_src, group| {
            group.bindings.retain(|(wid, _)| *wid != widget_id);
            !group.bindings.is_empty()
        });
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
    /// One-pass is load-bearing for correctness, not just an
    /// optimisation: a signal bound at both `AccessibilityOnly` and
    /// some visual level (e.g. `Button::label` registers at
    /// `RepaintOnly` for the inner TextWidget AND at
    /// `AccessibilityOnly` for the AT name) shares one underlying
    /// dirty flag across both bindings. If the visual flush cleared
    /// the flag before the a11y flush ran, the a11y check would see
    /// `false` and the AT cache would never refresh. Walking both
    /// buckets before any clearing fixes that.
    ///
    /// Cost is O(S) `is_dirty` calls (S = unique sources) plus O(D)
    /// widget-level promotions (D = bindings on dirty sources).
    /// Pre-optimization it was O(N) `is_dirty` calls (N = all
    /// bindings); on the catalog scene S≈30-40, N≈100-300.
    pub(crate) fn flush_all_dirty(&self) -> (Vec<(WidgetId, BindingLevel)>, bool) {
        let by_source = self.by_source.borrow();
        let mut dirty_map: HashMap<WidgetId, BindingLevel> = HashMap::new();
        let mut a11y_dirty = false;
        // Collect dirty groups first; clear at the end so a single
        // shared clear closure isn't called more than once. Ordering
        // doesn't matter — within a frame, clearing twice is
        // idempotent, but the per-call HashMap lookup still adds up.
        let mut to_clear: Vec<Rc<dyn Fn()>> = Vec::new();
        for group in by_source.values() {
            if !(group.is_dirty)() {
                continue;
            }
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
            to_clear.push(group.clear_dirty.clone());
        }
        for clear in to_clear {
            clear();
        }
        (dirty_map.into_iter().collect(), a11y_dirty)
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

    fn make_binding(level: BindingLevel, dirty: Rc<Cell<bool>>) -> Binding {
        let is_dirty = {
            let d = dirty.clone();
            Rc::new(move || d.get()) as Rc<dyn Fn() -> bool>
        };
        let clear_dirty = {
            let d = dirty.clone();
            Rc::new(move || d.set(false)) as Rc<dyn Fn()>
        };
        // WidgetId value doesn't matter for these tests; slotmap
        // default gives us a well-formed id.
        let id: WidgetId = slotmap::KeyData::from_ffi(1).into();
        Binding {
            widget_id: id,
            level,
            is_dirty,
            clear_dirty,
            // Every call creates a fresh cell → unique source id,
            // so existing tests that register multiple times don't
            // accidentally collapse through the new dedup path.
            source_id: Rc::as_ptr(&dirty) as *const () as usize,
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
        // `flush_all_dirty`, never the visual map. The unified flush
        // does still clear their dirty source — in production code
        // that's what we want, since the a11y flag is read at the same
        // call site.
        let reg = BindingRegistry::new();
        let dirty = Rc::new(Cell::new(true));
        reg.register(make_binding(BindingLevel::AccessibilityOnly, dirty.clone()));

        let (visual, a11y_dirty) = reg.flush_all_dirty();
        assert!(
            visual.is_empty(),
            "AccessibilityOnly must not appear in the visual dirty map"
        );
        assert!(a11y_dirty, "AccessibilityOnly binding must set a11y flag");
        assert!(!dirty.get(), "unified flush clears every dirty source");
    }

    #[test]
    fn flush_accessibility_dirty_returns_true_and_clears() {
        let reg = BindingRegistry::new();
        let dirty = Rc::new(Cell::new(true));
        reg.register(make_binding(BindingLevel::AccessibilityOnly, dirty.clone()));

        assert!(reg.flush_accessibility_dirty());
        assert!(
            !dirty.get(),
            "accessibility bindings must clear after drain"
        );
        assert!(
            !reg.flush_accessibility_dirty(),
            "second drain returns false (nothing dirty)"
        );
    }

    #[test]
    fn flush_all_dirty_drains_visual_and_a11y_in_one_pass() {
        // Regression: a Signal bound at both RepaintOnly and
        // AccessibilityOnly shares one underlying dirty flag. A single
        // `flush_all_dirty` call must surface both sides.
        let reg = BindingRegistry::new();
        let shared_dirty = Rc::new(Cell::new(true));
        // Two bindings, two levels, but one shared dirty source.
        reg.register(make_binding(
            BindingLevel::RepaintOnly,
            shared_dirty.clone(),
        ));
        reg.register(make_binding(
            BindingLevel::AccessibilityOnly,
            shared_dirty.clone(),
        ));

        let (visual, a11y_dirty) = reg.flush_all_dirty();
        assert_eq!(visual.len(), 1, "visual binding must fire");
        assert!(a11y_dirty, "a11y binding must fire from the same source");
        assert!(!shared_dirty.get(), "shared source cleared exactly once");
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
    fn phase4_unregister_for_widget_drops_empty_groups() {
        // After unregistering the only widget on a source, the
        // group is reclaimed so source slots can be reused without
        // memory growth across rebuilds.
        use crate::signal::Signal;
        let reg = BindingRegistry::new();
        let sig = Signal::new(0_i32);
        let id: WidgetId = slotmap::KeyData::from_ffi(44).into();

        sig.bind_to(id, &reg, BindingLevel::RepaintOnly);
        assert_eq!(reg.by_source.borrow().len(), 1);

        reg.unregister_for_widget(id);
        assert_eq!(reg.len(), 0);
        assert_eq!(
            reg.by_source.borrow().len(),
            0,
            "empty groups must be reclaimed"
        );
    }
}
