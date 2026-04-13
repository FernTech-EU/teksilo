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
        self.bindings.borrow_mut().push(binding);
    }

    /// Return widget IDs that need updating due to signal changes,
    /// along with the maximum binding level for each widget.
    /// Clears the dirty flags.
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
}

impl std::fmt::Debug for BindingRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BindingRegistry")
            .field("count", &self.bindings.borrow().len())
            .finish()
    }
}
