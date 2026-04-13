use slotmap::SlotMap;

use crate::environment::ThemeOverride;
use crate::event_handlers::EventHandlers;
use crate::event_source::{SubscriptionHandle, SubscriptionId};
use crate::gesture::{GestureArena, GestureEvent};
use crate::signal::{ObserverHandle, Prop};
use crate::widget::{CursorIcon, Widget};
use crate::widget_id::WidgetId;
use fern_canvas::RenderFrame;

/// Minimal placeholder widget used during composite rebuild and ID reservation.
#[derive(Debug)]
pub(crate) struct PlaceholderWidget;

impl Widget for PlaceholderWidget {
    fn size_that_fits(
        &self,
        _proposal: fern_canvas::SizeProposal,
        _ctx: &crate::widget::LayoutContext,
    ) -> fern_canvas::Size {
        fern_canvas::Size::ZERO
    }
}

/// Activation state for a widget in the arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationState {
    Active,
    Dormant,
    Destroyed,
}

/// Dirty flags for a widget.
#[derive(Debug, Clone, Copy, Default)]
pub struct DirtyFlags {
    pub needs_layout: bool,
    pub needs_paint: bool,
    /// When true, the widget's `build()` should be re-run to regenerate children.
    /// Set by `BindingLevel::Rebuild` bindings (data-driven widgets).
    pub needs_rebuild: bool,
}

/// A gesture binding: arena of recognizers + callback for when gestures fire.
#[allow(clippy::type_complexity)]
pub(crate) struct GestureBinding {
    pub arena: GestureArena,
    pub handler: Box<dyn FnMut(GestureEvent, &mut crate::widget::EventContext)>,
}

/// A node in the widget arena storing a widget and its metadata.
pub struct WidgetNode {
    pub widget: Box<dyn Widget>,
    pub parent: Option<WidgetId>,
    pub children: Vec<WidgetId>,
    pub activation: ActivationState,
    pub dirty: DirtyFlags,
    pub bounds: fern_canvas::Rect,
    pub(crate) gesture_binding: Option<GestureBinding>,
    pub(crate) theme_override: Option<ThemeOverride>,
    pub(crate) visible_state: Option<Prop<bool>>,
    pub(crate) enabled_state: Option<Prop<bool>>,
    pub(crate) alignment_override: Option<fern_tokens::Alignment>,
    /// When true, the paint pass clips child rendering to this widget's bounds.
    /// Set by scroll areas and overflow-hidden containers.
    pub clips_children: bool,
    /// Cached paint output for this widget (excludes children).
    /// Reused when `needs_paint` is false to avoid re-running `paint()`.
    pub(crate) cached_paint: Option<RenderFrame>,

    // --- V2 fields ---
    /// Attached event handlers (V2). Checked before widget.event() during dispatch.
    pub(crate) handlers: EventHandlers,
    /// Focusable override set via HandlerSet. Takes precedence over widget.is_focusable().
    pub(crate) node_focusable: Option<bool>,
    /// Tab index override set via HandlerSet.
    pub(crate) node_tab_index: Option<i32>,
    #[allow(dead_code)] // V2 API: spacer flag, set by Spacer during insertion
    pub(crate) node_is_spacer: bool,
    /// Cursor override set via HandlerSet.
    pub(crate) node_cursor: Option<CursorIcon>,
    /// Whether build() returned children (for rebuild on environment change).
    pub(crate) has_built_children: bool,
    /// RAII observer handles for effects registered during build().
    /// Dropped on rebuild or widget destruction.
    pub(crate) effect_handles: Vec<ObserverHandle>,
    /// Backend-event subscriptions registered during build() via
    /// `BuildContext::subscribe_event`. Each entry pairs a subscription id
    /// (used to remove the UI-side callback from `TreeAppContext`) with the
    /// opaque source-side handle whose `Drop` removes the subscriber from
    /// the source's internal registry. See architecture §9.4.5.
    pub(crate) subscription_handles: Vec<(SubscriptionId, SubscriptionHandle)>,
    /// Context menu factory — invoked on right-click to produce overlay content.
    pub(crate) context_menu_factory: Option<crate::widget_builder::ContextMenuFactory>,
}

impl std::fmt::Debug for WidgetNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WidgetNode")
            .field("widget", &self.widget)
            .field("parent", &self.parent)
            .field("children", &self.children)
            .field("activation", &self.activation)
            .field("dirty", &self.dirty)
            .field("bounds", &self.bounds)
            .field("has_gestures", &self.gesture_binding.is_some())
            .field("has_theme_override", &self.theme_override.is_some())
            .field("has_visible_state", &self.visible_state.is_some())
            .field("has_enabled_state", &self.enabled_state.is_some())
            .finish()
    }
}

/// Flat arena storage for all widgets, using SlotMap for O(1) access.
pub struct WidgetArena {
    nodes: SlotMap<WidgetId, WidgetNode>,
    /// Number of nodes with theme overrides. When zero, resolve_theme is O(1).
    pub(crate) theme_override_count: usize,
    /// Cached root widget IDs (widgets with no parent).
    cached_roots: Vec<WidgetId>,
    /// Whether the cached_roots list needs rebuilding.
    roots_dirty: bool,
}

impl WidgetArena {
    pub fn new() -> Self {
        Self {
            nodes: SlotMap::with_key(),
            theme_override_count: 0,
            cached_roots: Vec::new(),
            roots_dirty: true,
        }
    }

    /// Insert a widget into the arena as a root-level widget.
    pub fn insert(&mut self, widget: Box<dyn Widget>) -> WidgetId {
        self.roots_dirty = true;
        let children = widget.children();
        let id = self.nodes.insert(WidgetNode {
            widget,
            parent: None,
            children: Vec::new(),
            activation: ActivationState::Active,
            dirty: DirtyFlags {
                needs_layout: true,
                needs_paint: true,
                needs_rebuild: false,
            },
            bounds: fern_canvas::Rect::ZERO,
            gesture_binding: None,
            theme_override: None,
            visible_state: None,
            enabled_state: None,
            alignment_override: None,
            clips_children: false,
            cached_paint: None,
            handlers: EventHandlers::new(),
            node_focusable: None,
            node_tab_index: None,
            node_is_spacer: false,
            node_cursor: None,
            has_built_children: false,
            effect_handles: Vec::new(),
            subscription_handles: Vec::new(),
            context_menu_factory: None,
        });
        // Set up parent-child for declared children
        for &child_id in &children {
            if let Some(child_node) = self.nodes.get_mut(child_id) {
                child_node.parent = Some(id);
            }
        }
        if let Some(node) = self.nodes.get_mut(id) {
            node.children = children;
        }
        id
    }

    /// Insert a widget as a child of the given parent.
    pub fn insert_child(&mut self, parent: WidgetId, widget: Box<dyn Widget>) -> WidgetId {
        assert!(
            self.nodes.contains_key(parent),
            "insert_child() called with invalid parent WidgetId {parent:?}"
        );
        self.roots_dirty = true;
        let children = widget.children();
        let id = self.nodes.insert(WidgetNode {
            widget,
            parent: Some(parent),
            children: Vec::new(),
            activation: ActivationState::Active,
            dirty: DirtyFlags {
                needs_layout: true,
                needs_paint: true,
                needs_rebuild: false,
            },
            bounds: fern_canvas::Rect::ZERO,
            gesture_binding: None,
            theme_override: None,
            visible_state: None,
            enabled_state: None,
            alignment_override: None,
            clips_children: false,
            cached_paint: None,
            handlers: EventHandlers::new(),
            node_focusable: None,
            node_tab_index: None,
            node_is_spacer: false,
            node_cursor: None,
            has_built_children: false,
            effect_handles: Vec::new(),
            subscription_handles: Vec::new(),
            context_menu_factory: None,
        });
        // Set up parent-child for declared children
        for &child_id in &children {
            if let Some(child_node) = self.nodes.get_mut(child_id) {
                child_node.parent = Some(id);
            }
        }
        if let Some(node) = self.nodes.get_mut(id) {
            node.children = children;
        }
        if let Some(parent_node) = self.nodes.get_mut(parent) {
            parent_node.children.push(id);
        }
        id
    }

    pub fn get(&self, id: WidgetId) -> Option<&WidgetNode> {
        self.nodes.get(id)
    }

    pub fn get_mut(&mut self, id: WidgetId) -> Option<&mut WidgetNode> {
        self.nodes.get_mut(id)
    }

    pub fn children(&self, id: WidgetId) -> &[WidgetId] {
        self.nodes
            .get(id)
            .map(|n| n.children.as_slice())
            .unwrap_or(&[])
    }

    pub fn parent(&self, id: WidgetId) -> Option<WidgetId> {
        self.nodes.get(id).and_then(|n| n.parent)
    }

    pub fn bounds(&self, id: WidgetId) -> fern_canvas::Rect {
        self.nodes
            .get(id)
            .map(|n| n.bounds)
            .unwrap_or(fern_canvas::Rect::ZERO)
    }

    /// Get all root-level widget IDs (widgets with no parent).
    pub fn roots(&self) -> Vec<WidgetId> {
        if self.roots_dirty {
            // Fall back to scanning when cache is stale.
            // refresh_roots() should be called from layout() for the fast path.
            return self
                .nodes
                .iter()
                .filter(|(_, node)| node.parent.is_none())
                .map(|(id, _)| id)
                .collect();
        }
        self.cached_roots.clone()
    }

    /// Refresh the cached roots list. Call once per frame from layout().
    pub fn refresh_roots(&mut self) {
        if self.roots_dirty {
            self.cached_roots = self
                .nodes
                .iter()
                .filter(|(_, node)| node.parent.is_none())
                .map(|(id, _)| id)
                .collect();
            self.roots_dirty = false;
        }
    }

    /// Iterate over all active widget IDs.
    pub fn active_ids(&self) -> Vec<WidgetId> {
        self.nodes
            .iter()
            .filter(|(_, node)| node.activation == ActivationState::Active)
            .map(|(id, _)| id)
            .collect()
    }

    /// Set a widget subtree to dormant state (state preserved, not rendered).
    /// Recursively dormants all children.
    pub fn set_dormant(&mut self, id: WidgetId) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.activation = ActivationState::Dormant;
        }
        let children: Vec<WidgetId> = self.children(id).to_vec();
        for child in children {
            self.set_dormant(child);
        }
    }

    /// Activate a dormant widget subtree (triggers relayout and repaint).
    /// Recursively activates all children.
    pub fn activate(&mut self, id: WidgetId) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.activation = ActivationState::Active;
            node.dirty.needs_layout = true;
            node.dirty.needs_paint = true;
        }
        let children: Vec<WidgetId> = self.children(id).to_vec();
        for child in children {
            self.activate(child);
        }
    }

    /// Destroy a widget and remove it from the arena entirely.
    /// Recursively destroys all children. State is gone.
    pub fn destroy(&mut self, id: WidgetId) {
        self.roots_dirty = true;
        let children: Vec<WidgetId> = self.children(id).to_vec();
        for child in children {
            self.destroy(child);
        }
        // Remove from parent's children list
        if let Some(parent_id) = self.parent(id)
            && let Some(parent) = self.nodes.get_mut(parent_id)
        {
            parent.children.retain(|&c| c != id);
        }
        self.nodes.remove(id);
    }

    pub fn is_active(&self, id: WidgetId) -> bool {
        self.nodes
            .get(id)
            .map(|n| n.activation == ActivationState::Active)
            .unwrap_or(false)
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn mark_all_clean(&mut self) {
        for (_, node) in self.nodes.iter_mut() {
            node.dirty = DirtyFlags::default();
        }
    }

    pub fn any_needs_layout(&self) -> bool {
        self.nodes
            .values()
            .any(|n| n.activation == ActivationState::Active && n.dirty.needs_layout)
    }

    pub fn any_needs_paint(&self) -> bool {
        self.nodes
            .values()
            .any(|n| n.activation == ActivationState::Active && n.dirty.needs_paint)
    }

    pub fn mark_needs_paint(&mut self, id: WidgetId) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.dirty.needs_paint = true;
        }
    }

    pub fn mark_needs_layout(&mut self, id: WidgetId) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.dirty.needs_layout = true;
            node.dirty.needs_paint = true;
        }
    }

    /// Mark a widget as needing its `build()` re-run.
    /// Also marks for layout and paint since rebuilt children need both.
    pub fn mark_needs_rebuild(&mut self, id: WidgetId) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.dirty.needs_rebuild = true;
            node.dirty.needs_layout = true;
            node.dirty.needs_paint = true;
        }
    }

    /// Collect widgets that need their `build()` re-run (data-driven rebuild).
    /// Only returns active widgets with `has_built_children == true` and
    /// `needs_rebuild == true`.
    pub fn collect_needs_rebuild(&self) -> Vec<WidgetId> {
        self.nodes
            .iter()
            .filter(|(_, n)| {
                n.activation == ActivationState::Active
                    && n.has_built_children
                    && n.dirty.needs_rebuild
            })
            .map(|(id, _)| id)
            .collect()
    }

    /// Check all widgets with visible_state bindings and return
    /// (id, is_currently_active, should_be_visible) tuples.
    pub fn visibility_checks(&self) -> Vec<(WidgetId, bool, bool)> {
        let mut checks = Vec::new();
        for (id, node) in self.nodes.iter() {
            if let Some(ref state) = node.visible_state {
                let is_active = node.activation == ActivationState::Active;
                let should_be_visible = state.get();
                checks.push((id, is_active, should_be_visible));
            }
        }
        checks
    }

    /// Check if a widget is enabled (no enabled_state binding or state is true).
    pub fn is_enabled(&self, id: WidgetId) -> bool {
        self.nodes
            .get(id)
            .map(|n| n.enabled_state.as_ref().map(|s| s.get()).unwrap_or(true))
            .unwrap_or(true)
    }

    /// Set a per-child alignment override on a widget.
    pub fn set_alignment_override(&mut self, id: WidgetId, alignment: fern_tokens::Alignment) {
        if let Some(node) = self.get_mut(id) {
            node.alignment_override = Some(alignment);
        }
    }

    /// Mark a widget as clipping its children (scroll area, overflow hidden).
    pub fn set_clips_children(&mut self, id: WidgetId, clips: bool) {
        if let Some(node) = self.get_mut(id) {
            node.clips_children = clips;
        }
    }

    /// Apply a `HandlerSet` to an existing node, transferring handlers and metadata.
    /// Called from `BuildContext::apply_self_handlers()` during `build()`.
    pub(crate) fn apply_handler_set(
        &mut self,
        id: WidgetId,
        handler_set: crate::widget_builder::HandlerSet,
    ) {
        if let Some(node) = self.get_mut(id) {
            let existing_handlers = std::mem::take(&mut node.handlers);
            node.handlers = existing_handlers.merge(handler_set.handlers);
            if let Some(focusable) = handler_set.focusable {
                node.node_focusable = Some(focusable);
            }
            if let Some(tab_index) = handler_set.tab_index {
                node.node_tab_index = Some(tab_index);
            }
            if let Some(cursor) = handler_set.cursor {
                node.node_cursor = Some(cursor);
            }
            if let Some(clips) = handler_set.clips_children {
                node.clips_children = clips;
            }
            if handler_set.context_menu_factory.is_some() {
                node.context_menu_factory = handler_set.context_menu_factory;
            }
        }
    }

    /// Get a widget's alignment override, if any.
    pub fn alignment_override(&self, id: WidgetId) -> Option<fern_tokens::Alignment> {
        self.get(id)?.alignment_override
    }

    /// Temporarily take the widget box out of a node (for rebuild).
    /// The node remains in the arena with a placeholder.
    pub fn take_widget(&mut self, id: WidgetId) -> Option<Box<dyn Widget>> {
        let node = self.nodes.get_mut(id)?;
        // Replace with a minimal placeholder
        let taken = std::mem::replace(&mut node.widget, Box::new(PlaceholderWidget));
        Some(taken)
    }

    /// Restore a widget box that was previously taken out.
    pub fn restore_widget(&mut self, id: WidgetId, widget: Box<dyn Widget>) {
        if let Some(node) = self.nodes.get_mut(id) {
            node.widget = widget;
        }
    }

    /// Walk up the parent chain from `id` and mark each ancestor as needing layout.
    /// Called when a relayout-level binding changes, since a child's size change
    /// may affect its parent's size, and so on up to the root.
    pub fn mark_ancestors_need_layout(&mut self, id: WidgetId) {
        let mut current = self.parent(id);
        while let Some(pid) = current {
            if let Some(node) = self.get_mut(pid) {
                node.dirty.needs_layout = true;
                node.dirty.needs_paint = true;
            }
            current = self.parent(pid);
        }
    }

    /// Mark all widgets as needing layout and paint (e.g. after a theme change).
    /// Also clears per-widget paint caches since the visual output is stale.
    pub fn mark_all_dirty(&mut self) {
        for (_, node) in self.nodes.iter_mut() {
            node.dirty.needs_layout = true;
            node.dirty.needs_paint = true;
            node.cached_paint = None;
        }
    }

    /// Resolve the effective theme for a widget by walking ancestors and
    /// applying any theme overrides encountered along the way.
    /// The base theme is the tree-level default.
    pub fn resolve_theme(&self, id: WidgetId, base: &fern_tokens::Theme) -> fern_tokens::Theme {
        // Fast path: if no widget has a theme override, skip the ancestor walk.
        if self.theme_override_count == 0 {
            return base.clone();
        }

        // Collect ancestor chain from root to widget
        let mut chain = vec![id];
        let mut current = self.parent(id);
        while let Some(pid) = current {
            chain.push(pid);
            current = self.parent(pid);
        }
        chain.reverse(); // root first

        let mut theme = base.clone();
        for nid in chain {
            if let Some(node) = self.nodes.get(nid)
                && let Some(ovr) = &node.theme_override
            {
                (ovr.func)(&mut theme);
            }
        }
        theme
    }
}

impl Default for WidgetArena {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_widgets::FillWidget;

    #[test]
    fn insert_and_retrieve() {
        let mut arena = WidgetArena::new();
        let id = arena.insert(Box::new(FillWidget::new()));
        assert!(arena.get(id).is_some());
        assert_eq!(arena.len(), 1);
    }

    #[test]
    fn new_widget_is_dirty() {
        let mut arena = WidgetArena::new();
        let id = arena.insert(Box::new(FillWidget::new()));
        let node = arena.get(id).unwrap();
        assert!(node.dirty.needs_layout);
        assert!(node.dirty.needs_paint);
    }

    #[test]
    fn roots_returns_parentless_widgets() {
        let mut arena = WidgetArena::new();
        let root = arena.insert(Box::new(FillWidget::new()));
        let _child = arena.insert_child(root, Box::new(FillWidget::new()));
        let roots = arena.roots();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0], root);
    }
}
