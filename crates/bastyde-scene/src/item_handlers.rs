//! Per-item event handlers, cursor and tooltip overrides.
//!
//! [`SceneItemHandlerSet`] is the lightweight-tier counterpart to
//! widget-level [`HandlerSet`](bastyde_core::widget_builder::HandlerSet).
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

use bastyde_canvas::Point;
use bastyde_core::event::{ButtonMask, Modifiers, PointerButton};
use bastyde_core::widget::{CursorIcon, EventContext};

/// Box of an item-level event closure with a single non-event
/// argument (used for `on_hover`'s `bool` payload).
type ItemHandler<A> = Rc<dyn Fn(A, &mut EventContext)>;

/// Click-style gesture envelope for scene items. Mirrors the
/// widget-tier [`bastyde_core::gesture::TapEvent`] but with the
/// position in **scene** coordinates instead of widget-local. Used
/// by the tap / double-tap / triple-tap / long-press / context-menu
/// handlers on [`SceneItemHandlerSet`].
///
/// `#[non_exhaustive]` so future additions (e.g. tap count,
/// stylus pressure) can land without breaking match patterns.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct SceneTapEvent {
    /// Click position in **scene** coordinates. The SceneView's
    /// dispatch converts the raw screen-pixel pointer position
    /// through the view transform before populating this field,
    /// so handlers see the same frame their item's geometry is in.
    pub position_scene: Point,
    /// Which button finalised the gesture.
    pub button: PointerButton,
    /// Modifier keys held at dispatch time.
    pub modifiers: Modifiers,
}

impl SceneTapEvent {
    /// Construct one by hand. Useful for tests; dispatch builds
    /// these from the live pointer event in `SceneView`.
    pub fn new(position_scene: Point, button: PointerButton, modifiers: Modifiers) -> Self {
        Self {
            position_scene,
            button,
            modifiers,
        }
    }
}

/// Rich tap-family handler storage type — what every Set's
/// `on_tap` / `on_double_tap` / `on_context_menu` field actually
/// holds after Unit 7. The Point-only convenience setter
/// [`SceneItemHandlerSet::on_tap`] wraps caller closures with a
/// shim that extracts `event.position_scene`, so legacy call
/// sites compile unchanged.
type SceneTapHandler = Rc<dyn Fn(&SceneTapEvent, &mut EventContext)>;

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
#[derive(Clone)]
pub struct SceneItemHandlerSet {
    /// Tap (single click). Stored as the rich `SceneTapEvent`
    /// form internally; the simpler [`Self::on_tap`] setter wraps
    /// `Fn(Point, ...)` callers in a shim that extracts
    /// `event.position_scene`.
    pub on_tap: Option<SceneTapHandler>,
    /// Double-tap. Storage only — the SceneView's dispatch site
    /// doesn't synthesise double-tap recognition yet; this field
    /// stays unset-on-fire until a future unit wires the
    /// recognizer in. Treat as a forward-declared slot.
    pub on_double_tap: Option<SceneTapHandler>,
    /// Hover transitions: `bool` is `true` on enter, `false` on
    /// leave.
    pub on_hover: Option<ItemHandler<bool>>,
    /// Right-click / OS-native context-menu trigger.
    pub on_context_menu: Option<SceneTapHandler>,
    /// Which pointer buttons count as a "tap" / context-menu
    /// invocation. Default [`ButtonMask::PRIMARY`] for tap; the
    /// SECONDARY button always routes through `on_context_menu`
    /// regardless of this mask. Items wanting middle-click-as-tap
    /// extend the mask: `accept_tap_buttons(PRIMARY | MIDDLE)`.
    /// Mirrors widget-tier `accept_tap_buttons(...)`.
    pub accept_tap_buttons: ButtonMask,
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

impl Default for SceneItemHandlerSet {
    fn default() -> Self {
        Self {
            on_tap: None,
            on_double_tap: None,
            on_hover: None,
            on_context_menu: None,
            accept_tap_buttons: ButtonMask::PRIMARY,
            cursor: None,
            tooltip: None,
            accepts_drops: false,
        }
    }
}

impl SceneItemHandlerSet {
    /// An empty handler set — every closure unset, no cursor or
    /// tooltip.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tap callback. Simpler `Fn(Point, &mut ctx)`
    /// signature for callers that only need the click position;
    /// internally wraps in a shim that extracts
    /// `event.position_scene`. For modifier-aware handlers (Shift-
    /// click selection, Ctrl-click toggle, etc.) use
    /// [`Self::on_tap_event`] which exposes the full
    /// [`SceneTapEvent`].
    pub fn on_tap<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(Point, &mut EventContext) + 'static,
    {
        let f = Rc::new(f);
        self.on_tap = Some(Rc::new(move |ev: &SceneTapEvent, ctx| {
            f(ev.position_scene, ctx);
        }));
        self
    }

    /// Register a tap callback that receives the full
    /// [`SceneTapEvent`] — scene-coord position, button, modifiers.
    /// Use for modifier-aware patterns (`Shift+click extends
    /// selection`, `Ctrl+click toggles`, middle-click handlers
    /// once paired with `accept_tap_buttons`).
    pub fn on_tap_event<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(&SceneTapEvent, &mut EventContext) + 'static,
    {
        self.on_tap = Some(Rc::new(f));
        self
    }

    /// Register a double-tap callback (Point-only shim — see
    /// [`Self::on_tap`]). **Not wired yet:** the SceneView's
    /// dispatch doesn't recognise double-tap; the field is stored
    /// but never fired. A future unit wires the recognizer.
    pub fn on_double_tap<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(Point, &mut EventContext) + 'static,
    {
        let f = Rc::new(f);
        self.on_double_tap = Some(Rc::new(move |ev: &SceneTapEvent, ctx| {
            f(ev.position_scene, ctx);
        }));
        self
    }

    /// Rich-event variant of [`Self::on_double_tap`].
    pub fn on_double_tap_event<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(&SceneTapEvent, &mut EventContext) + 'static,
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

    /// Register a context-menu callback (right-click). Point-only
    /// shim; see [`Self::on_context_menu_event`] for the rich
    /// variant.
    pub fn on_context_menu<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(Point, &mut EventContext) + 'static,
    {
        let f = Rc::new(f);
        self.on_context_menu = Some(Rc::new(move |ev: &SceneTapEvent, ctx| {
            f(ev.position_scene, ctx);
        }));
        self
    }

    /// Rich-event variant of [`Self::on_context_menu`].
    pub fn on_context_menu_event<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(&SceneTapEvent, &mut EventContext) + 'static,
    {
        self.on_context_menu = Some(Rc::new(f));
        self
    }

    /// Mask of pointer buttons that should be treated as a tap
    /// for this item. Default [`ButtonMask::PRIMARY`]. Right-click
    /// (`SECONDARY`) always routes through `on_context_menu`
    /// regardless of this mask.
    pub fn accept_tap_buttons(&mut self, mask: ButtonMask) -> &mut Self {
        self.accept_tap_buttons = mask;
        self
    }

    /// Override the cursor icon shown over this item.
    pub fn cursor(&mut self, c: CursorIcon) -> &mut Self {
        self.cursor = Some(c);
        self
    }

    /// Set a tooltip string. Accepts anything convertible into
    /// [`LocalizedString`](bastyde_i18n::LocalizedString) — most commonly
    /// `tr!(...)` for translated copy. Plain strings auto-convert.
    /// The text is resolved eagerly at builder time.
    pub fn tooltip(&mut self, t: impl Into<bastyde_i18n::LocalizedString>) -> &mut Self {
        let ls: bastyde_i18n::LocalizedString = t.into();
        self.tooltip = Some(ls.resolve_now());
        self
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
            .field(
                "on_context_menu",
                &self.on_context_menu.as_ref().map(|_| "..."),
            )
            .field("accept_tap_buttons", &self.accept_tap_buttons)
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
