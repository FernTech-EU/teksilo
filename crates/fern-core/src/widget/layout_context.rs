use crate::widget_id::WidgetId;

use super::LayoutResponse;

/// Context available during layout.
pub struct LayoutContext<'a> {
    pub theme: &'a crate::styles::Theme,
    pub layout_direction: crate::environment::LayoutDirection,
    /// Text backend for accurate text measurement during layout.
    pub text_backend: Option<&'a std::rc::Rc<std::cell::RefCell<dyn fern_canvas::TextBackend>>>,
    /// Arena reference for querying child widget sizes.
    pub(crate) arena: Option<&'a crate::arena::WidgetArena>,
    /// Optional bundle of read-only tree state (focus, shortcuts,
    /// overlays). Carried through layout for debug-tooling consumers
    /// like the inspector's Focus / Shortcuts / Overlays tabs. `None`
    /// in test contexts.
    pub(crate) extras: Option<LayoutExtras<'a>>,
}

/// Read-only handles to tree-level state that some widgets (notably
/// the debug inspector) want to query from `layout_response`. Threaded
/// through the recursive layout pass alongside the arena.
#[derive(Clone, Copy)]
pub(crate) struct LayoutExtras<'a> {
    pub focused: Option<WidgetId>,
    pub shortcut_registry: Option<&'a crate::shortcut::ShortcutRegistry>,
    pub overlay_manager: Option<&'a crate::overlay::OverlayManager>,
}

impl<'a> LayoutContext<'a> {
    /// Create a LayoutContext for testing (no arena access).
    pub fn for_testing(theme: &'a crate::styles::Theme) -> Self {
        Self {
            theme,
            layout_direction: crate::environment::LayoutDirection::LeftToRight,
            text_backend: None,
            arena: None,
            extras: None,
        }
    }

    /// The currently focused widget id, if any. Returns `None` when
    /// the layout pass is unrelated to a tree (test contexts).
    pub fn focused(&self) -> Option<WidgetId> {
        self.extras.as_ref().and_then(|e| e.focused)
    }

    /// Borrow the tree's shortcut registry. Returns `None` outside a
    /// real layout pass. Intended for read-only inspection by the
    /// debug inspector.
    pub fn shortcut_registry(&self) -> Option<&crate::shortcut::ShortcutRegistry> {
        self.extras.as_ref().and_then(|e| e.shortcut_registry)
    }

    /// Borrow the tree's overlay manager. Returns `None` outside a
    /// real layout pass. Intended for read-only inspection by the
    /// debug inspector.
    pub fn overlay_manager(&self) -> Option<&crate::overlay::OverlayManager> {
        self.extras.as_ref().and_then(|e| e.overlay_manager)
    }

    /// Query a child widget's full layout response (wanted size + flex weight).
    /// Returns None if the child doesn't exist, is dormant, or the arena is not available.
    pub fn child_layout_response(
        &self,
        child_id: WidgetId,
        proposal: fern_canvas::SizeProposal,
    ) -> Option<LayoutResponse> {
        let arena = self.arena?;
        if !arena.is_active(child_id) {
            return None;
        }
        let node = arena.get(child_id)?;
        Some(node.widget.layout_response(proposal, self))
    }

    /// Query a child widget's wanted size only (drops the flex weight).
    /// Convenience over [`child_layout_response`](Self::child_layout_response).
    pub fn child_size(
        &self,
        child_id: WidgetId,
        proposal: fern_canvas::SizeProposal,
    ) -> Option<fern_canvas::Size> {
        self.child_layout_response(child_id, proposal)
            .map(|r| r.size)
    }

    /// Query the laid-out bounds of any active widget. Returns `None`
    /// when the arena is not available (test contexts) — otherwise
    /// returns the widget's current bounds (`Rect::ZERO` if unknown).
    /// Useful for inspector-style widgets that need to mirror another
    /// widget's geometry into a `Signal` during the layout pass.
    pub fn widget_bounds(&self, id: WidgetId) -> Option<fern_canvas::Rect> {
        let arena = self.arena?;
        if !arena.is_active(id) {
            return None;
        }
        Some(arena.bounds(id))
    }

    /// Hit-test the active widget tree at `point` and return the
    /// deepest widget under it. Honors `event_pass_through`. The
    /// `exclude` argument lets the caller skip a specific subtree
    /// (e.g. the inspector's picker overlay so it doesn't pick
    /// itself). Returns `None` outside layout (no arena available).
    pub fn widget_at_point(
        &self,
        point: fern_canvas::Point,
        exclude: Option<WidgetId>,
    ) -> Option<WidgetId> {
        let arena = self.arena?;
        arena.hit_test_at(point, exclude)
    }

    /// Borrow the underlying arena. Returns `None` outside a layout
    /// pass (test contexts). Intended for read-only introspection by
    /// debug tooling (the inspector's tree view) — use the typed
    /// accessors above when possible.
    pub fn arena(&self) -> Option<&crate::arena::WidgetArena> {
        self.arena
    }

    /// Query a child's per-widget alignment override, if any.
    pub fn child_alignment(&self, child_id: WidgetId) -> Option<fern_tokens::Alignment> {
        let arena = self.arena?;
        arena.alignment_override(child_id)
    }

    /// Whether the layout direction is right-to-left.
    pub fn is_rtl(&self) -> bool {
        self.layout_direction == crate::environment::LayoutDirection::RightToLeft
    }
}
