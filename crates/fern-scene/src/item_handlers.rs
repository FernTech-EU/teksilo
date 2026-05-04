//! Per-item event handlers, cursor and tooltip overrides.
//!
//! [`SceneItemHandlerSet`] is the lightweight-tier counterpart to
//! widget-level [`HandlerSet`](fern_core::widget_builder::HandlerSet).
//! It carries optional closures the [`SceneView`](crate::SceneView)
//! invokes when pointer / hover / context-menu events land on the
//! item, plus per-item cursor and tooltip overrides.
//!
//! Apps attach handlers via [`Scene::set_item_handlers`] /
//! [`Scene::handlers_mut`] after `add_item`:
//!
//! ```ignore
//! let id = scene.add_item(rect, Point::ZERO);
//! scene.handlers_mut(id).unwrap()
//!     .on_tap(|_pt, ctx| ctx.send_intent(AppIntent::OpenCard))
//!     .cursor(CursorIcon::Pointer)
//!     .tooltip("Open card");
//! ```

use std::rc::Rc;

use fern_canvas::Point;
use fern_core::widget::{CursorIcon, EventContext};

/// Box of an item-level event closure, parameterised by the
/// argument type the closure receives.
type ItemHandler<A> = Rc<dyn Fn(A, &mut EventContext)>;

/// What a [`SceneView`](crate::SceneView)'s on-canvas pointer drag
/// does in empty space.
///
/// * [`DragMode::NoDrag`] — nothing happens. Useful for embedded
///   read-only diagrams.
/// * [`DragMode::ScrollHandDrag`] — left-click-drag pans the view.
///   Item-level on-drag handlers are bypassed; the canvas grabs
///   the gesture unconditionally.
/// * [`DragMode::RubberBand`] (default) — drag-on-empty-space
///   creates a marquee that selects items inside on release.
///   Drag-on-an-item dispatches to that item's drag handler if
///   wired (R3 currently honours `IS_DRAGGABLE` for drag-to-move).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DragMode {
    NoDrag,
    ScrollHandDrag,
    #[default]
    RubberBand,
}

/// Per-item event closures + cursor + tooltip + drop acceptance.
///
/// Closures are stored as `Rc<dyn Fn>` so cloning the handler set
/// is cheap; the SceneView clones into its dispatch path.
#[derive(Default, Clone)]
pub struct SceneItemHandlerSet {
    /// Tap (single click). Receives the click position in **scene**
    /// coordinates and the framework's [`EventContext`].
    pub on_tap: Option<ItemHandler<Point>>,
    /// Double-tap.
    pub on_double_tap: Option<ItemHandler<Point>>,
    /// Hover transitions: `bool` is `true` on enter, `false` on
    /// leave.
    pub on_hover: Option<ItemHandler<bool>>,
    /// Right-click / OS-native context-menu trigger.
    pub on_context_menu: Option<ItemHandler<Point>>,
    /// Cursor icon shown while the pointer is over this item.
    /// Overrides the SceneView default.
    pub cursor: Option<CursorIcon>,
    /// Tooltip string (already-resolved). The SceneView's hover
    /// machinery surfaces this through the standard overlay
    /// manager. R3 ships the cursor and tooltip plumbing; the
    /// overlay activation is wired in R5 with `ensure_visible`.
    pub tooltip: Option<String>,
    /// Whether the item accepts dropped payloads. R3 sets the
    /// flag; the cross-tier drop pipeline integrates in R4.
    pub accepts_drops: bool,
}

impl SceneItemHandlerSet {
    /// An empty handler set — every closure unset, no cursor or
    /// tooltip.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tap callback. The closure receives the click
    /// position in scene coords.
    pub fn on_tap<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(Point, &mut EventContext) + 'static,
    {
        self.on_tap = Some(Rc::new(f));
        self
    }

    /// Register a double-tap callback.
    pub fn on_double_tap<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(Point, &mut EventContext) + 'static,
    {
        self.on_double_tap = Some(Rc::new(f));
        self
    }

    /// Register a hover callback. Receives `true` on enter,
    /// `false` on leave.
    pub fn on_hover<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(bool, &mut EventContext) + 'static,
    {
        self.on_hover = Some(Rc::new(f));
        self
    }

    /// Register a context-menu callback (right-click).
    pub fn on_context_menu<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(Point, &mut EventContext) + 'static,
    {
        self.on_context_menu = Some(Rc::new(f));
        self
    }

    /// Override the cursor icon shown over this item.
    pub fn cursor(&mut self, c: CursorIcon) -> &mut Self {
        self.cursor = Some(c);
        self
    }

    /// Set a tooltip string. Accepts anything convertible into
    /// [`LocalizedString`](fern_i18n::LocalizedString) — most commonly
    /// `tr!(...)` for translated copy. Plain strings auto-convert.
    /// The text is resolved eagerly at builder time.
    pub fn tooltip(
        &mut self,
        t: impl Into<fern_i18n::LocalizedString>,
    ) -> &mut Self {
        let ls: fern_i18n::LocalizedString = t.into();
        self.tooltip = Some(ls.resolve_now());
        self
    }

    /// Untranslated twin of [`tooltip`](Self::tooltip). Wraps the
    /// argument via [`LocalizedString::literal`](fern_i18n::LocalizedString::literal)
    /// — a grep-marker for call sites that intentionally bypass i18n.
    #[doc(hidden)]
    pub fn tooltip_literal(&mut self, t: impl Into<String>) -> &mut Self {
        self.tooltip(fern_i18n::LocalizedString::literal(t))
    }

    /// Mark whether the item accepts dropped payloads.
    pub fn accepts_drops(&mut self, accepts: bool) -> &mut Self {
        self.accepts_drops = accepts;
        self
    }
}

impl std::fmt::Debug for SceneItemHandlerSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneItemHandlerSet")
            .field("on_tap", &self.on_tap.as_ref().map(|_| "..."))
            .field("on_double_tap", &self.on_double_tap.as_ref().map(|_| "..."))
            .field("on_hover", &self.on_hover.as_ref().map(|_| "..."))
            .field("on_context_menu", &self.on_context_menu.as_ref().map(|_| "..."))
            .field("cursor", &self.cursor)
            .field("tooltip", &self.tooltip)
            .field("accepts_drops", &self.accepts_drops)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_handler_set_has_no_callbacks() {
        let h = SceneItemHandlerSet::new();
        assert!(h.on_tap.is_none());
        assert!(h.on_hover.is_none());
        assert!(h.on_context_menu.is_none());
        assert!(h.cursor.is_none());
        assert!(h.tooltip.is_none());
        assert!(!h.accepts_drops);
    }

    #[test]
    fn cursor_and_tooltip_round_trip() {
        let mut h = SceneItemHandlerSet::new();
        h.cursor(CursorIcon::Pointer).tooltip("hello");
        assert_eq!(h.cursor, Some(CursorIcon::Pointer));
        assert_eq!(h.tooltip.as_deref(), Some("hello"));
    }

    #[test]
    fn drag_mode_default_is_rubber_band() {
        assert_eq!(DragMode::default(), DragMode::RubberBand);
    }
}
