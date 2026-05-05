//! Binding registry: the dirty-tracking infrastructure shared between
//! `Signal<T>` instances and `WidgetTree`.
//!
//! Despite the module name (kept for historical reasons), this file no
//! longer contains any state primitive — `Signal<T>` in `signal.rs` is
//! the only reactive type. The registry, its binding entries, and the
//! `BindingLevel` enum live here because they are the shared vocabulary
//! between signals and the widget tree.

use std::cell::RefCell;
use std::rc::Rc;

use crate::widget_id::WidgetId;

/// Dirty-tracking granularity for a property binding.
/// Determined by the primitive widget implementor, not the consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingLevel {
    /// Visual-only change (color, opacity). Marks the widget for repaint;
    /// layout is skipped.
    RepaintOnly,
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
    /// [`Signal::source_id`](crate::signal::Signal::source_id).
    /// Used by [`BindingRegistry::register`] to dedup repeated
    /// `bind_to` calls within a single build cycle.
    pub source_id: usize,
}

/// Shared registry of all active property bindings.
#[derive(Clone, Default)]
pub struct BindingRegistry {
    pub(crate) bindings: Rc<RefCell<Vec<Binding>>>,
}

impl BindingRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn register(&self, binding: Binding) {
        // Dedup: if an entry already exists for this
        // (widget_id, source_id, bucket) tuple, merge levels instead
        // of pushing a duplicate. AccessibilityOnly bindings live in
        // a different "bucket" from visual bindings (they flush
        // through separate paths) so we don't collapse across that
        // axis.
        let same_bucket = |existing: &Binding| -> bool {
            existing.widget_id == binding.widget_id
                && existing.source_id == binding.source_id
                && is_a11y_only(existing.level) == is_a11y_only(binding.level)
        };
        let mut bindings = self.bindings.borrow_mut();
        if let Some(existing) = bindings.iter_mut().find(|b| same_bucket(b)) {
            existing.level = promote_level(existing.level, binding.level);
            return;
        }
        bindings.push(binding);
    }

    /// Drop every binding targeting `widget_id`. Called by the widget
    /// tree before a widget rebuilds (so `build()` can re-register a
    /// fresh, deduplicated set) and on destroy (so a dead widget's
    /// bindings no longer keep source-signal references alive or
    /// accumulate across the lifetime of the app).
    pub(crate) fn unregister_for_widget(&self, widget_id: WidgetId) {
        self.bindings
            .borrow_mut()
            .retain(|b| b.widget_id != widget_id);
    }

    /// Number of live bindings. Exposed for tests that verify
    /// cleanup does not accumulate entries across rebuilds.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.bindings.borrow().len()
    }

    /// Return widget IDs that need updating due to signal changes,
    /// along with the maximum VISUAL binding level for each widget.
    /// Clears the dirty flags for processed visual bindings.
    ///
    /// `AccessibilityOnly` bindings are NOT included here — they are
    /// drained separately by [`flush_accessibility_dirty`] because the
    /// accessibility dirty bit is orthogonal to the repaint / relayout /
    /// rebuild pipeline and doesn't belong to any widget in particular
    /// (the whole `WidgetTree::a11y_dirty` flag is a single tree-wide
    /// boolean).
    pub(crate) fn flush_dirty(&self) -> Vec<(WidgetId, BindingLevel)> {
        let bindings = self.bindings.borrow();
        let mut dirty_map: std::collections::HashMap<WidgetId, BindingLevel> =
            std::collections::HashMap::new();
        // Collect all dirty bindings first, then clear. Multiple bindings may
        // share the same underlying dirty flag (e.g. derived signals from the
        // same source). Clearing immediately would cause later bindings to miss
        // the change.
        let mut to_clear: Vec<&Rc<dyn Fn()>> = Vec::new();
        for b in bindings.iter() {
            if matches!(b.level, BindingLevel::AccessibilityOnly) {
                // Skip — drained by `flush_accessibility_dirty`.
                continue;
            }
            if (b.is_dirty)() {
                let entry = dirty_map.entry(b.widget_id).or_insert(b.level);
                // Promote to the highest priority level seen for this widget.
                // Priority: Rebuild > Relayout > RepaintOnly.
                match b.level {
                    BindingLevel::Rebuild => *entry = BindingLevel::Rebuild,
                    BindingLevel::Relayout if *entry == BindingLevel::RepaintOnly => {
                        *entry = BindingLevel::Relayout;
                    }
                    _ => {}
                }
                to_clear.push(&b.clear_dirty);
            }
        }
        for clear in to_clear {
            clear();
        }
        dirty_map.into_iter().collect()
    }

    /// Return `true` if any binding registered at
    /// `BindingLevel::AccessibilityOnly` has fired since the last
    /// flush. Clears the dirty flags on those bindings. Called from
    /// [`WidgetTree::process_state_changes`] to flip `a11y_dirty`
    /// whenever a signal bound at this level changes — notably the
    /// rich text editor's `document_version` on every text edit.
    pub(crate) fn flush_accessibility_dirty(&self) -> bool {
        let bindings = self.bindings.borrow();
        let mut any_dirty = false;
        let mut to_clear: Vec<&Rc<dyn Fn()>> = Vec::new();
        for b in bindings.iter() {
            if !matches!(b.level, BindingLevel::AccessibilityOnly) {
                continue;
            }
            if (b.is_dirty)() {
                any_dirty = true;
                to_clear.push(&b.clear_dirty);
            }
        }
        for clear in to_clear {
            clear();
        }
        any_dirty
    }
}

/// Priority order for visual binding levels — `Rebuild` dominates
/// `Relayout` dominates `RepaintOnly`. `AccessibilityOnly` lives in
/// its own bucket and is never compared against visual levels.
fn promote_level(existing: BindingLevel, incoming: BindingLevel) -> BindingLevel {
    use BindingLevel::*;
    match (existing, incoming) {
        (Rebuild, _) | (_, Rebuild) => Rebuild,
        (Relayout, _) | (_, Relayout) => Relayout,
        (RepaintOnly, _) | (_, RepaintOnly) => RepaintOnly,
        (AccessibilityOnly, AccessibilityOnly) => AccessibilityOnly,
    }
}

fn is_a11y_only(level: BindingLevel) -> bool {
    matches!(level, BindingLevel::AccessibilityOnly)
}

impl std::fmt::Debug for BindingRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BindingRegistry")
            .field("count", &self.bindings.borrow().len())
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
    fn accessibility_only_binding_skipped_by_flush_dirty() {
        let reg = BindingRegistry::new();
        let dirty = Rc::new(Cell::new(true));
        reg.register(make_binding(BindingLevel::AccessibilityOnly, dirty.clone()));

        let visual = reg.flush_dirty();
        assert!(
            visual.is_empty(),
            "flush_dirty must not include AccessibilityOnly bindings"
        );
        // Dirty bit stays set; accessibility-only drain happens separately.
        assert!(dirty.get());
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
    fn flush_accessibility_dirty_ignores_visual_levels() {
        let reg = BindingRegistry::new();
        let repaint_dirty = Rc::new(Cell::new(true));
        let a11y_dirty = Rc::new(Cell::new(false));
        reg.register(make_binding(
            BindingLevel::RepaintOnly,
            repaint_dirty.clone(),
        ));
        reg.register(make_binding(
            BindingLevel::AccessibilityOnly,
            a11y_dirty.clone(),
        ));

        assert!(
            !reg.flush_accessibility_dirty(),
            "only a11y binding is clean, result must be false"
        );
        // Visual binding stays dirty because flush_accessibility_dirty
        // doesn't touch it.
        assert!(repaint_dirty.get());
    }

    #[test]
    fn signal_bind_to_accessibility_only_propagates_via_registry() {
        // End-to-end: a real Signal<T> bound at AccessibilityOnly
        // fires flush_accessibility_dirty without affecting flush_dirty.
        let reg = BindingRegistry::new();
        let sig = Signal::new(0_u64);
        let id: WidgetId = slotmap::KeyData::from_ffi(7).into();
        sig.bind_to(id, &reg, BindingLevel::AccessibilityOnly);

        // Fresh binding is not dirty yet.
        assert!(!reg.flush_accessibility_dirty());

        sig.set(1);
        assert!(reg.flush_accessibility_dirty());
        // Subsequent drain is clean again.
        assert!(!reg.flush_accessibility_dirty());
        // Signal changes don't leak into flush_dirty.
        assert!(reg.flush_dirty().is_empty());
    }
}
