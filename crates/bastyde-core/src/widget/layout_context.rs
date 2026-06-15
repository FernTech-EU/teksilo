// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use crate::widget_id::WidgetId;

use super::LayoutResponse;

/// Context available during layout.
pub struct LayoutContext<'a> {
    pub theme: &'a crate::styles::Theme,
    pub layout_direction: crate::environment::LayoutDirection,
    /// Host window HiDPI device scale (physical px per logical px). The layout
    /// pass is otherwise fully logical, and the renderer applies this scale at
    /// the vertex stage — so **ordinary widgets must ignore this**. It exists
    /// only as the escape hatch for widgets that bridge to a device-pixel OS
    /// resource (e.g. a `WebView` sizing its native subview, which on some
    /// toolkits — WebKitGTK on X11 — ignores fractional scaling and needs
    /// device pixels). 1.0 in headless / test contexts.
    pub scale_factor: f32,
    /// Text backend for accurate text measurement during layout.
    pub text_backend: Option<&'a std::rc::Rc<std::cell::RefCell<dyn bastyde_canvas::TextBackend>>>,
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
            scale_factor: 1.0,
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
        proposal: bastyde_canvas::SizeProposal,
    ) -> Option<LayoutResponse> {
        // Routed through the arena's per-pass memoization cache so the
        // main-then-cross queries that height-for-width negotiation issues do
        // not recompute the same `(child, proposal)` repeatedly. The cache
        // handles the active-state check and the `cacheable_layout()` opt-out.
        let arena = self.arena?;
        arena.cached_layout_response(child_id, proposal, self)
    }

    /// Measure a widget's intrinsic size for `proposal`, **regardless of
    /// activation** — works even for dormant/collapsed widgets (and their
    /// dormant subtrees), unlike [`child_size`](Self::child_size) /
    /// [`child_layout_response`](Self::child_layout_response), which return
    /// `None` for inactive widgets.
    ///
    /// Intended for adaptive layouts that hide some children but still need
    /// their size to decide when to reveal them — e.g. an overflow `Toolbar`
    /// collapsing actions into a chevron menu. Runs uncached and re-entrant-
    /// safe; calls `layout_response`, which must be idempotent.
    pub fn measure_intrinsic(
        &self,
        id: WidgetId,
        proposal: bastyde_canvas::SizeProposal,
    ) -> Option<bastyde_canvas::Size> {
        self.arena?.measure_intrinsic(id, proposal, self)
    }

    /// Query a child widget's wanted size only (drops the flex weight).
    /// Convenience over [`child_layout_response`](Self::child_layout_response).
    pub fn child_size(
        &self,
        child_id: WidgetId,
        proposal: bastyde_canvas::SizeProposal,
    ) -> Option<bastyde_canvas::Size> {
        self.child_layout_response(child_id, proposal)
            .map(|r| r.size)
    }

    /// Query the laid-out bounds of any active widget. Returns `None`
    /// when the arena is not available (test contexts) — otherwise
    /// returns the widget's current bounds (`Rect::ZERO` if unknown).
    /// Useful for inspector-style widgets that need to mirror another
    /// widget's geometry into a `Signal` during the layout pass.
    pub fn widget_bounds(&self, id: WidgetId) -> Option<bastyde_canvas::Rect> {
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
        point: bastyde_canvas::Point,
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
    pub fn child_alignment(&self, child_id: WidgetId) -> Option<bastyde_tokens::Alignment> {
        let arena = self.arena?;
        arena.alignment_override(child_id)
    }

    /// Whether the layout direction is right-to-left.
    pub fn is_rtl(&self) -> bool {
        self.layout_direction == crate::environment::LayoutDirection::RightToLeft
    }
}
