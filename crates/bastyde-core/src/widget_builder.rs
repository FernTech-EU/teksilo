//! WidgetBuilder trait — blanket-implemented for all Widget types.
//!
//! Provides attached event handler methods and framework-level properties.
//! Each method wraps the widget in a `WidgetWithHandlers<W>` that stores
//! the handlers and metadata alongside the widget. When the widget is
//! inserted into the arena, the handler set is extracted and applied to
//! the `WidgetNode`.
//!
//! The four click-style handlers (`on_tap` / `on_double_tap` /
//! `on_triple_tap` / `on_long_press`) all receive a borrowed
//! [`crate::gesture::TapEvent`] (position + button + modifiers) and
//! default to [`crate::event::ButtonMask::PRIMARY`] acceptance. Widen
//! that filter via the matching `accept_*_buttons(...)` knob — see the
//! "Event System" section in `docs/events-and-gestures.md` for the
//! full contract and examples.

use bastyde_canvas::Point;

use crate::event::{ButtonMask, EventResponse, WidgetEvent};
use crate::event_handlers::EventHandlers;
use crate::gesture::{DragPhase, PinchPhase, SwipeDirection, TapEvent};
use crate::widget::{CursorIcon, EventContext, Widget};
use crate::widget_id::WidgetId;

// ---------------------------------------------------------------------------
// Accessibility overrides
// ---------------------------------------------------------------------------

/// Subtree visibility / merge mode applied by the accessibility tree walker.
///
/// Set via `WidgetBuilder::access_exclude_subtree()` /
/// `access_merge_subtree()`. The walker honors the mode after the parent
/// node has been emitted, before recursing into descendants.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccessSubtreeMode {
    /// Normal walk — descendants emitted as their own AT nodes.
    #[default]
    Inherit,
    /// Descendants pruned from the AT tree entirely. Parent node still
    /// emitted normally. Equivalent to Flutter's `excludeSemantics: true`.
    Exclude,
    /// Descendants' labels / descriptions / values / actions are
    /// concatenated into the parent's emitted node, then descendants are
    /// pruned. The parent reads as a single AT element. Equivalent to
    /// Flutter's `mergeAllDescendants: true` and SwiftUI's
    /// `.accessibilityElement(children: .combine)`.
    Merge,
}

/// Builder-level accessibility overrides.
///
/// Carried on `HandlerSet` during builder-chain construction, mirrored
/// onto `WidgetNode::access_overrides` at arena insertion (parallel to
/// `clips_children` / `cursor` / `focus_within_signal`), then applied by
/// the accessibility tree walker after the inner widget's
/// `accessibility(&self, builder)` runs.
///
/// User-visible string fields store eagerly-resolved `String`. The
/// translated path goes through `impl From<LocalizedString> for String`
/// in `bastyde-i18n` — `.access_label(tr!("save"))` resolves the
/// `LocalizedString` once at builder time and stores the result here.
/// Locale changes rebuild the composite, which re-runs the builder
/// chain and picks up new translations. The `_literal` builder
/// variants are `#[doc(hidden)]` grep markers for explicitly
/// untranslated call sites — same convention as `Button::new_literal`.
#[derive(Default)]
pub struct AccessibilityOverrides {
    // -- Tier 1: labeling / state -----------------------------------------
    pub label: Option<String>,
    pub description: Option<String>,
    pub value: Option<String>,
    pub role: Option<accesskit::Role>,
    pub hidden: Option<bool>,
    pub disabled: Option<bool>,

    // -- Tier 2: relationships / live / identity --------------------------
    pub identifier: Option<String>,
    pub controls: Vec<WidgetId>,
    pub described_by: Vec<WidgetId>,
    pub labelled_by: Vec<WidgetId>,
    pub live: Option<accesskit::Live>,
    pub aria_current: Option<accesskit::AriaCurrent>,
    /// Pre-formatted shortcut announcement string (e.g. `"Ctrl+S"`).
    /// Used by `access_shortcut_literal`. For chords routed through a
    /// `Shortcut` registration, prefer `access_shortcut_id` (stored
    /// in `shortcut_id`) so the announcement tracks rebinds.
    pub shortcut: Option<String>,
    /// Registered shortcut id (e.g. `"app.save"`). The accessibility
    /// walker resolves the current keystroke from
    /// `WidgetTree::shortcut_registry()` at AT-build time and writes
    /// the formatted string to `Node::keyboard_shortcut`. Refreshes
    /// automatically when the user rebinds (the registry's `version`
    /// signal triggers a re-sync).
    pub shortcut_id: Option<String>,
    pub has_popup: Option<accesskit::HasPopup>,
    pub orientation: Option<accesskit::Orientation>,

    // -- Tier 3: numeric / actions / escape hatch -------------------------
    pub numeric_value: Option<f64>,
    pub min_numeric_value: Option<f64>,
    pub max_numeric_value: Option<f64>,
    pub numeric_step: Option<f64>,

    /// Standard `accesskit::Action` advertisements with their handlers.
    /// Dispatched by `event_dispatch_impl.rs` when handling
    /// `WidgetEvent::AccessAction`, layered on top of any
    /// user-installed `on_access_action` / `on_access_action_request`
    /// handlers (both fire for the same dispatched event).
    pub actions: Vec<(accesskit::Action, Box<dyn FnMut(&mut EventContext)>)>,

    /// Actions to remove from the widget-emitted action list (called
    /// after the widget's `accessibility()` runs, before custom-action
    /// emission).
    pub removed_actions: Vec<accesskit::Action>,

    /// Custom-named actions (SwiftUI `.accessibilityAction(named:_:)`).
    /// Each entry pairs a resolved description string with a handler.
    /// Index in the vec is the stable `i32` `CustomAction::id` exposed
    /// to AT software.
    pub custom_actions: Vec<(String, Box<dyn FnMut(&mut EventContext)>)>,

    /// Final escape hatch — invoked **last** in `apply()` with full
    /// `&mut AccessNodeBuilder` access (including `inner_mut()`). Used
    /// for sub-node surgery (synthetic children) and for cases the
    /// typed surface doesn't cover.
    pub customize: Option<Box<dyn Fn(&mut crate::accessibility::AccessNodeBuilder)>>,
}

impl std::fmt::Debug for AccessibilityOverrides {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccessibilityOverrides")
            .field("label", &self.label)
            .field("description", &self.description)
            .field("value", &self.value)
            .field("role", &self.role)
            .field("hidden", &self.hidden)
            .field("disabled", &self.disabled)
            .field("identifier", &self.identifier)
            .field("shortcut", &self.shortcut)
            .field("shortcut_id", &self.shortcut_id)
            .field("controls_len", &self.controls.len())
            .field("described_by_len", &self.described_by.len())
            .field("labelled_by_len", &self.labelled_by.len())
            .field("actions_len", &self.actions.len())
            .field("removed_actions", &self.removed_actions)
            .field("custom_actions_len", &self.custom_actions.len())
            .finish()
    }
}

impl AccessibilityOverrides {
    /// Apply the override scalar / list fields onto a builder. Called by
    /// the accessibility tree walker after the inner widget's
    /// `accessibility(&self, builder)` runs and before the framework
    /// finalizes the node.
    pub(crate) fn apply(&self, b: &mut crate::accessibility::AccessNodeBuilder) {
        use crate::accessibility::widget_id_to_node_id;

        if let Some(ref s) = self.label {
            b.set_name(s.clone());
        }
        if let Some(ref s) = self.description {
            b.set_description(s.clone());
        }
        if let Some(ref s) = self.value {
            b.set_value(s.clone());
        }
        if let Some(role) = self.role {
            b.set_role(role);
        }
        match self.hidden {
            Some(true) => b.set_hidden(),
            Some(false) => b.clear_hidden(),
            None => {}
        }
        match self.disabled {
            Some(true) => b.set_disabled(),
            Some(false) => b.clear_disabled(),
            None => {}
        }
        if let Some(ref s) = self.identifier {
            b.set_author_id(s.clone());
        }
        for &id in &self.controls {
            b.push_controlled(widget_id_to_node_id(id));
        }
        for &id in &self.described_by {
            b.push_described_by(widget_id_to_node_id(id));
        }
        for &id in &self.labelled_by {
            b.push_labelled_by(widget_id_to_node_id(id));
        }
        if let Some(live) = self.live {
            b.set_live(live);
        }
        if let Some(c) = self.aria_current {
            b.set_aria_current(c);
        }
        if let Some(ref s) = self.shortcut {
            b.set_keyboard_shortcut(s.clone());
        }
        // `shortcut_id` resolution happens in the accessibility tree
        // walker (where the `ShortcutRegistry` is reachable) — see
        // `accessibility_impl::build_accessibility_recursive`.
        if let Some(p) = self.has_popup {
            b.set_has_popup(p);
        }
        if let Some(o) = self.orientation {
            b.set_orientation(o);
        }
        if let Some(v) = self.numeric_value {
            b.set_numeric_value(v);
        }
        if let Some(v) = self.min_numeric_value {
            b.set_min_numeric_value(v);
        }
        if let Some(v) = self.max_numeric_value {
            b.set_max_numeric_value(v);
        }
        if let Some(v) = self.numeric_step {
            b.set_numeric_value_step(v);
        }
        // Suppression first, then advertisement — so `access_remove_action`
        // can prune what the widget emitted, but a subsequent
        // `access_action(same_action, ...)` re-advertises with the
        // override-installed handler.
        for &a in &self.removed_actions {
            b.remove_action(a);
        }
        for (action, _) in &self.actions {
            b.add_action(*action);
        }
        if !self.custom_actions.is_empty() {
            let custom: Vec<accesskit::CustomAction> = self
                .custom_actions
                .iter()
                .enumerate()
                .map(|(i, (label, _))| accesskit::CustomAction {
                    id: i as i32,
                    description: label.clone().into(),
                })
                .collect();
            b.set_custom_actions(custom);
        }
        if let Some(ref f) = self.customize {
            f(b);
        }
    }
}

// ---------------------------------------------------------------------------
// HandlerSet — temporary storage before arena insertion
// ---------------------------------------------------------------------------

/// Temporary storage for handlers and metadata accumulated via builder
/// methods. Transferred to the `WidgetNode` during arena insertion.
/// Type alias for a context-menu content factory.
///
/// The factory is invoked on every right-click that lands on a widget
/// owning the factory (or on a descendant whose nearest ancestor with
/// a factory is this one). It receives:
///
/// - `position`: pointer position in widget-local coordinates of the
///   factory-owning widget. Useful when the menu's contents depend on
///   *what* was right-clicked (a row in a list, a node in a tree, an
///   item under a hit-test, …).
/// - `ctx`: a full [`EventContext`], so the factory can read window
///   state, query app state, send intents (e.g. for analytics), or
///   update Signals before the menu mounts.
///
/// The factory returns:
///
/// - `Some(widget)` to mount `widget` as the menu overlay anchored at
///   the factory-owning widget, placed at `position`.
/// - `None` to **decline this right-click**. The framework continues
///   walking up the parent chain looking for the next ancestor with a
///   factory. This lets a widget conditionally suppress its own menu
///   without uninstalling the factory.
pub type ContextMenuFactory = Box<dyn Fn(Point, &mut EventContext) -> Option<Box<dyn Widget>>>;

pub struct HandlerSet {
    pub(crate) handlers: EventHandlers,
    pub(crate) focusable: Option<bool>,
    pub(crate) tab_index: Option<i32>,
    pub(crate) cursor: Option<CursorIcon>,
    pub(crate) clips_children: Option<bool>,
    /// When `Some(false)`, the widget opts out of OS input-method (IME)
    /// composition while focused — used by secure / password fields so
    /// the OS preedit / candidate window can't surface plaintext. The
    /// platform IME wiring reads the focused node's `ime_allowed` flag
    /// at focus-change time. `None` inherits the default (`true`).
    pub(crate) ime_allowed: Option<bool>,
    /// When `Some(true)`, the widget node is invisible to pointer
    /// hit-testing — events fall through to whatever sits behind it.
    /// Used by the debug inspector's overlay widgets.
    pub(crate) event_pass_through: Option<bool>,
    pub(crate) context_menu_factory: Option<ContextMenuFactory>,
    /// User-bound signal that the framework writes whenever the
    /// focused widget is a strict descendant of this node. See
    /// [`HandlerSet::focus_within`].
    pub(crate) focus_within: Option<crate::signal::Signal<bool>>,
    /// User-bound signal that the framework writes whenever the
    /// hovered widget is a strict descendant of this node. See
    /// [`HandlerSet::hover_within`].
    pub(crate) hover_within: Option<crate::signal::Signal<bool>>,
    /// Builder-level accessibility overrides. Mirrored to
    /// `WidgetNode::access_overrides` at insertion. Action callbacks
    /// (`actions`, `custom_actions`) are dispatched by
    /// `event_dispatch_impl.rs` when handling
    /// `WidgetEvent::AccessAction`, in addition to the user's
    /// `on_access_action` / `on_access_action_request` handlers — so
    /// builder order doesn't matter.
    pub(crate) access: Option<Box<AccessibilityOverrides>>,
    /// Subtree visibility / merge mode. Mirrored to
    /// `WidgetNode::access_subtree`.
    pub(crate) access_subtree: Option<AccessSubtreeMode>,
}

impl HandlerSet {
    /// Create an empty handler set for use in `BuildContext::apply_self_handlers()`.
    pub fn new() -> Self {
        Self {
            handlers: EventHandlers::new(),
            focusable: None,
            tab_index: None,
            cursor: None,
            clips_children: None,
            ime_allowed: None,
            event_pass_through: None,
            context_menu_factory: None,
            focus_within: None,
            hover_within: None,
            access: None,
            access_subtree: None,
        }
    }

    /// Get a `&mut` to the override block, lazily allocating it on first
    /// access. Used by all `access_*` builder methods.
    pub(crate) fn access_mut(&mut self) -> &mut AccessibilityOverrides {
        self.access
            .get_or_insert_with(|| Box::new(AccessibilityOverrides::default()))
    }

    // -- Builder methods (mirror WidgetWithHandlers) --

    /// Set the on_tap handler. The closure receives a borrowed
    /// [`TapEvent`](crate::gesture::TapEvent) carrying the position in
    /// widget-local coordinates, the finalising mouse button, and the
    /// modifier state at that moment.
    ///
    /// Default acceptance is [`ButtonMask::PRIMARY`] — left-click only.
    /// Use [`accept_tap_buttons`](Self::accept_tap_buttons) to widen
    /// the set if you need right-click, middle-click, or auxiliary
    /// buttons to fire this handler.
    pub fn on_tap(mut self, f: impl FnMut(&TapEvent, &mut EventContext) + 'static) -> Self {
        self.handlers.on_tap = Some(Box::new(f));
        self
    }

    /// Set the on_double_tap handler. See [`on_tap`](Self::on_tap) for
    /// the callback contract.
    pub fn on_double_tap(mut self, f: impl FnMut(&TapEvent, &mut EventContext) + 'static) -> Self {
        self.handlers.on_double_tap = Some(Box::new(f));
        self
    }

    /// Set the on_triple_tap handler — fires on the third click within the
    /// recognizer's window (same 300 ms / 10 px defaults as double tap).
    /// Runs independently of `on_double_tap` via cooperative gesture
    /// recognizers (`GestureRecognizer::resets_on_peer_recognition`).
    pub fn on_triple_tap(mut self, f: impl FnMut(&TapEvent, &mut EventContext) + 'static) -> Self {
        self.handlers.on_triple_tap = Some(Box::new(f));
        self
    }

    /// Set the on_long_press handler. The callback receives a borrowed
    /// [`TapEvent`](crate::gesture::TapEvent) whose modifiers are
    /// captured from the held `Down` (since long-press recognises on a
    /// timer before any `Up`).
    pub fn on_long_press(mut self, f: impl FnMut(&TapEvent, &mut EventContext) + 'static) -> Self {
        self.handlers.on_long_press = Some(Box::new(f));
        self
    }

    /// Restrict (or extend) the set of pointer buttons that fire
    /// [`on_tap`](Self::on_tap). Default is [`ButtonMask::PRIMARY`]
    /// (left-click only). Pass `ButtonMask::ALL` or
    /// `ButtonMask::PRIMARY | ButtonMask::SECONDARY`, etc.
    pub fn accept_tap_buttons(mut self, mask: impl Into<ButtonMask>) -> Self {
        self.handlers.tap_buttons = Some(mask.into());
        self
    }

    /// Restrict (or extend) the set of pointer buttons that fire
    /// [`on_double_tap`](Self::on_double_tap). Default
    /// [`ButtonMask::PRIMARY`].
    pub fn accept_double_tap_buttons(mut self, mask: impl Into<ButtonMask>) -> Self {
        self.handlers.double_tap_buttons = Some(mask.into());
        self
    }

    /// Restrict (or extend) the set of pointer buttons that fire
    /// [`on_triple_tap`](Self::on_triple_tap). Default
    /// [`ButtonMask::PRIMARY`].
    pub fn accept_triple_tap_buttons(mut self, mask: impl Into<ButtonMask>) -> Self {
        self.handlers.triple_tap_buttons = Some(mask.into());
        self
    }

    /// Restrict (or extend) the set of pointer buttons that fire
    /// [`on_long_press`](Self::on_long_press). Default
    /// [`ButtonMask::PRIMARY`].
    pub fn accept_long_press_buttons(mut self, mask: impl Into<ButtonMask>) -> Self {
        self.handlers.long_press_buttons = Some(mask.into());
        self
    }

    /// Set the on_hover handler.
    pub fn on_hover(mut self, f: impl FnMut(bool, &mut EventContext) + 'static) -> Self {
        self.handlers.on_hover = Some(Box::new(f));
        self
    }

    /// Set the on_key handler.
    pub fn on_key(
        mut self,
        f: impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static,
    ) -> Self {
        self.handlers.on_key = Some(Box::new(f));
        self
    }

    /// Set the strict-ancestor key preview handler. Fires on every
    /// ancestor of the focused widget (root → parent-of-target)
    /// before the focused widget's `on_key` runs. Return
    /// `EventResponse::Handled` to consume the event.
    pub fn on_key_preview(
        mut self,
        f: impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static,
    ) -> Self {
        self.handlers.on_key_preview = Some(Box::new(f));
        self
    }

    /// Set the on_drag handler (gesture-based drag). The closure receives
    /// a [`DragPhase`] per architecture §28.3 — `Started`, then zero or
    /// more `Moved`, then `Ended`.
    pub fn on_drag(mut self, f: impl FnMut(DragPhase, &mut EventContext) + 'static) -> Self {
        self.handlers.on_drag = Some(Box::new(f));
        self
    }

    /// Set the on_swipe handler. Fires once per swipe with the direction
    /// and velocity (pixels/second).
    pub fn on_swipe(
        mut self,
        f: impl FnMut(SwipeDirection, f32, &mut EventContext) + 'static,
    ) -> Self {
        self.handlers.on_swipe = Some(Box::new(f));
        self
    }

    /// Set the on_pinch handler. On desktop the phases are produced from
    /// OS trackpad gestures (winit `TouchpadMagnify` / `RotationGesture`).
    pub fn on_pinch(mut self, f: impl FnMut(PinchPhase, &mut EventContext) + 'static) -> Self {
        self.handlers.on_pinch = Some(Box::new(f));
        self
    }

    /// Set the on_focus handler.
    pub fn on_focus(mut self, f: impl FnMut(bool, &mut EventContext) + 'static) -> Self {
        self.handlers.on_focus = Some(Box::new(f));
        self
    }

    /// Set the on_pointer_event handler (low-level escape hatch).
    pub fn on_pointer_event(
        mut self,
        f: impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static,
    ) -> Self {
        self.handlers.on_pointer_event = Some(Box::new(f));
        self
    }

    /// Set the on_scroll handler.
    pub fn on_scroll(
        mut self,
        f: impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static,
    ) -> Self {
        self.handlers.on_scroll = Some(Box::new(f));
        self
    }

    /// Set the on_access_action handler.
    pub fn on_access_action(
        mut self,
        f: impl FnMut(accesskit::Action, &mut EventContext) -> EventResponse + 'static,
    ) -> Self {
        self.handlers.on_access_action = Some(Box::new(f));
        self
    }

    /// Set the full AccessKit action-request handler. Receives the
    /// action, target NodeId (may be a synthetic widget-emitted
    /// child), and optional `ActionData` payload (e.g.
    /// `SetTextSelection(TextSelection)` or `Value(Box<str>)`).
    /// When this slot is set it's called INSTEAD of
    /// `on_access_action` for the same event.
    pub fn on_access_action_request(
        mut self,
        f: impl FnMut(
            accesskit::Action,
            accesskit::NodeId,
            Option<accesskit::ActionData>,
            &mut EventContext,
        ) -> EventResponse
        + 'static,
    ) -> Self {
        self.handlers.on_access_action_request = Some(Box::new(f));
        self
    }

    /// Set the focusable flag.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = Some(focusable);
        self
    }

    /// Set the cursor icon.
    pub fn cursor(mut self, cursor: CursorIcon) -> Self {
        self.cursor = Some(cursor);
        self
    }

    /// Set the clips_children flag.
    pub fn clips_children(mut self, clips: bool) -> Self {
        self.clips_children = Some(clips);
        self
    }

    /// Opt this node in or out of OS input-method (IME) composition
    /// while it is focused. Defaults to `true` (IME allowed). Secure /
    /// password fields set `false` so the OS preedit / candidate window
    /// cannot surface plaintext. The platform IME layer reads the
    /// focused node's flag at focus-change time.
    pub fn ime_allowed(mut self, allowed: bool) -> Self {
        self.ime_allowed = Some(allowed);
        self
    }

    /// Make the widget invisible to pointer hit-testing. With
    /// `pass_through = true`, pointer events traverse this node as if
    /// it were not there — useful for purely decorative overlays that
    /// must not absorb clicks (the debug inspector's `HighlightLayer`
    /// and `HoverProbe` use this).
    pub fn event_pass_through(mut self, pass_through: bool) -> Self {
        self.event_pass_through = Some(pass_through);
        self
    }

    /// Bind a user-owned [`Signal<bool>`] that the framework will set
    /// to `true` whenever the focused widget is a *strict descendant*
    /// of this node, and `false` otherwise. Useful for unified focus
    /// halos around composite widgets (a chat composer that highlights
    /// when its `RichTextEditor` or "Send" button is focused, a
    /// `Panel` wrapping a `SpinBox`, etc).
    ///
    /// Strict-ancestors only — a widget that *is* itself focused does
    /// not also see its own `focus_within` signal flipped to `true`.
    /// Combine with `on_focus` if you want both behaviours.
    pub fn focus_within(mut self, signal: crate::signal::Signal<bool>) -> Self {
        self.focus_within = Some(signal);
        self
    }

    /// Bind a user-owned [`Signal<bool>`] that the framework will set
    /// to `true` whenever the hovered widget is a *strict descendant*
    /// of this node. Symmetric to [`focus_within`](Self::focus_within).
    pub fn hover_within(mut self, signal: crate::signal::Signal<bool>) -> Self {
        self.hover_within = Some(signal);
        self
    }

    /// Set a context-menu factory. See [`ContextMenuFactory`] for the
    /// full contract: the closure receives the click position
    /// (widget-local) and a full [`EventContext`], and returns
    /// `Some(menu)` to mount or `None` to decline (falling through to
    /// the nearest ancestor with a factory).
    pub fn context_menu(
        mut self,
        factory: impl Fn(Point, &mut EventContext) -> Option<Box<dyn Widget>> + 'static,
    ) -> Self {
        self.context_menu_factory = Some(Box::new(factory));
        self
    }

    /// Set the drag hover handler. Called when a drag payload hovers over this widget.
    /// Return `DropFeedback` to indicate acceptance and visual feedback.
    pub fn on_drag_hover(
        mut self,
        f: impl FnMut(
            &crate::drag_payload::DragPayload,
            bastyde_canvas::Point,
            &mut EventContext,
        ) -> crate::drag_state::DropFeedback
        + 'static,
    ) -> Self {
        self.handlers.on_drag_hover = Some(Box::new(f));
        self
    }

    /// Set the drag-leave handler. Fires when a drag that was over this
    /// widget moves to another target, completes (drop on any target), or
    /// is cancelled. Widgets that stash transient feedback state in
    /// `on_drag_hover` must clear it here.
    pub fn on_drag_leave(mut self, f: impl FnMut(&mut EventContext) + 'static) -> Self {
        self.handlers.on_drag_leave = Some(Box::new(f));
        self
    }

    /// Set the per-frame drag-tick handler. Fires once per frame while a
    /// drag is active and this widget is the current drop target. The
    /// closure receives the current pointer position in widget-local
    /// coordinates. Use for behaviours that must keep running even when
    /// the pointer is stationary — viewport-edge auto-scroll and
    /// spring-loaded folders.
    pub fn on_drag_tick(
        mut self,
        f: impl FnMut(bastyde_canvas::Point, &mut EventContext) + 'static,
    ) -> Self {
        self.handlers.on_drag_tick = Some(Box::new(f));
        self
    }

    /// Set the drop handler. Called when a payload is dropped on this widget.
    /// Return `true` if the drop was accepted.
    pub fn on_drop(
        mut self,
        f: impl FnMut(crate::drag_payload::DragPayload, bastyde_canvas::Point, &mut EventContext) -> bool
        + 'static,
    ) -> Self {
        self.handlers.on_drop = Some(Box::new(f));
        self
    }
}

impl Default for HandlerSet {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for HandlerSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HandlerSet")
            .field("handlers", &self.handlers)
            .field("focusable", &self.focusable)
            .field("tab_index", &self.tab_index)
            .field("cursor", &self.cursor)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// WidgetWithHandlers<W> — wrapper storing widget + accumulated handlers
// ---------------------------------------------------------------------------

/// A widget wrapped with attached event handlers and framework metadata.
/// Created by calling builder methods from `WidgetBuilder` on any widget.
pub struct WidgetWithHandlers<W: Widget> {
    pub(crate) widget: W,
    pub(crate) handler_set: HandlerSet,
}

impl<W: Widget> WidgetWithHandlers<W> {
    fn new(widget: W) -> Self {
        Self {
            widget,
            handler_set: HandlerSet::new(),
        }
    }

    /// Take the handler set out, leaving defaults.
    #[allow(dead_code)] // V2 API: used during widget insertion to extract handlers
    pub(crate) fn take_handler_set(&mut self) -> HandlerSet {
        std::mem::take(&mut self.handler_set)
    }

    // -- Gesture handlers --

    pub fn on_tap(mut self, f: impl FnMut(&TapEvent, &mut EventContext) + 'static) -> Self {
        self.handler_set.handlers.on_tap = Some(Box::new(f));
        self
    }

    pub fn on_double_tap(mut self, f: impl FnMut(&TapEvent, &mut EventContext) + 'static) -> Self {
        self.handler_set.handlers.on_double_tap = Some(Box::new(f));
        self
    }

    pub fn on_triple_tap(mut self, f: impl FnMut(&TapEvent, &mut EventContext) + 'static) -> Self {
        self.handler_set.handlers.on_triple_tap = Some(Box::new(f));
        self
    }

    pub fn on_long_press(mut self, f: impl FnMut(&TapEvent, &mut EventContext) + 'static) -> Self {
        self.handler_set.handlers.on_long_press = Some(Box::new(f));
        self
    }

    /// Restrict (or extend) the set of pointer buttons that fire
    /// `on_tap`. Default is [`ButtonMask::PRIMARY`].
    pub fn accept_tap_buttons(mut self, mask: impl Into<ButtonMask>) -> Self {
        self.handler_set.handlers.tap_buttons = Some(mask.into());
        self
    }

    /// Restrict (or extend) the set of pointer buttons that fire
    /// `on_double_tap`. Default [`ButtonMask::PRIMARY`].
    pub fn accept_double_tap_buttons(mut self, mask: impl Into<ButtonMask>) -> Self {
        self.handler_set.handlers.double_tap_buttons = Some(mask.into());
        self
    }

    /// Restrict (or extend) the set of pointer buttons that fire
    /// `on_triple_tap`. Default [`ButtonMask::PRIMARY`].
    pub fn accept_triple_tap_buttons(mut self, mask: impl Into<ButtonMask>) -> Self {
        self.handler_set.handlers.triple_tap_buttons = Some(mask.into());
        self
    }

    /// Restrict (or extend) the set of pointer buttons that fire
    /// `on_long_press`. Default [`ButtonMask::PRIMARY`].
    pub fn accept_long_press_buttons(mut self, mask: impl Into<ButtonMask>) -> Self {
        self.handler_set.handlers.long_press_buttons = Some(mask.into());
        self
    }

    pub fn on_drag(mut self, f: impl FnMut(DragPhase, &mut EventContext) + 'static) -> Self {
        self.handler_set.handlers.on_drag = Some(Box::new(f));
        self
    }

    pub fn on_swipe(
        mut self,
        f: impl FnMut(SwipeDirection, f32, &mut EventContext) + 'static,
    ) -> Self {
        self.handler_set.handlers.on_swipe = Some(Box::new(f));
        self
    }

    pub fn on_pinch(mut self, f: impl FnMut(PinchPhase, &mut EventContext) + 'static) -> Self {
        self.handler_set.handlers.on_pinch = Some(Box::new(f));
        self
    }

    // -- Focus and keyboard --

    pub fn on_focus(mut self, f: impl FnMut(bool, &mut EventContext) + 'static) -> Self {
        self.handler_set.handlers.on_focus = Some(Box::new(f));
        self
    }

    pub fn on_key(
        mut self,
        f: impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static,
    ) -> Self {
        self.handler_set.handlers.on_key = Some(Box::new(f));
        self
    }

    /// Set the strict-ancestor key preview handler. See
    /// [`HandlerSet::on_key_preview`].
    pub fn on_key_preview(
        mut self,
        f: impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static,
    ) -> Self {
        self.handler_set.handlers.on_key_preview = Some(Box::new(f));
        self
    }

    pub fn focusable(mut self, focusable: bool) -> Self {
        self.handler_set.focusable = Some(focusable);
        self
    }

    pub fn tab_index(mut self, index: i32) -> Self {
        self.handler_set.tab_index = Some(index);
        self
    }

    // -- Pointer (low-level escape hatch) --

    pub fn on_pointer_event(
        mut self,
        f: impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static,
    ) -> Self {
        self.handler_set.handlers.on_pointer_event = Some(Box::new(f));
        self
    }

    pub fn on_hover(mut self, f: impl FnMut(bool, &mut EventContext) + 'static) -> Self {
        self.handler_set.handlers.on_hover = Some(Box::new(f));
        self
    }

    pub fn cursor(mut self, cursor: CursorIcon) -> Self {
        self.handler_set.cursor = Some(cursor);
        self
    }

    // -- Scroll --

    pub fn on_scroll(
        mut self,
        f: impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static,
    ) -> Self {
        self.handler_set.handlers.on_scroll = Some(Box::new(f));
        self
    }

    // -- Accessibility actions --

    pub fn on_access_action(
        mut self,
        f: impl FnMut(accesskit::Action, &mut EventContext) -> EventResponse + 'static,
    ) -> Self {
        self.handler_set.handlers.on_access_action = Some(Box::new(f));
        self
    }

    pub fn on_access_action_request(
        mut self,
        f: impl FnMut(
            accesskit::Action,
            accesskit::NodeId,
            Option<accesskit::ActionData>,
            &mut EventContext,
        ) -> EventResponse
        + 'static,
    ) -> Self {
        self.handler_set.handlers.on_access_action_request = Some(Box::new(f));
        self
    }

    // -- Framework-level properties --

    pub fn clips_children(mut self, clips: bool) -> Self {
        self.handler_set.clips_children = Some(clips);
        self
    }

    /// Opt this node in or out of OS IME composition while focused. See
    /// [`HandlerSet::ime_allowed`].
    pub fn ime_allowed(mut self, allowed: bool) -> Self {
        self.handler_set.ime_allowed = Some(allowed);
        self
    }

    /// Make the widget invisible to pointer hit-testing. See
    /// [`HandlerSet::event_pass_through`].
    pub fn event_pass_through(mut self, pass_through: bool) -> Self {
        self.handler_set.event_pass_through = Some(pass_through);
        self
    }

    /// Set a context-menu factory. See
    /// [`HandlerSet::context_menu`] for the full contract.
    pub fn context_menu(
        mut self,
        factory: impl Fn(Point, &mut EventContext) -> Option<Box<dyn Widget>> + 'static,
    ) -> Self {
        self.handler_set.context_menu_factory = Some(Box::new(factory));
        self
    }

    /// Bind a `Signal<bool>` the framework writes when a strict
    /// descendant has focus. See [`HandlerSet::focus_within`].
    pub fn focus_within(mut self, signal: crate::signal::Signal<bool>) -> Self {
        self.handler_set.focus_within = Some(signal);
        self
    }

    /// Bind a `Signal<bool>` the framework writes when a strict
    /// descendant is hovered. See [`HandlerSet::hover_within`].
    pub fn hover_within(mut self, signal: crate::signal::Signal<bool>) -> Self {
        self.handler_set.hover_within = Some(signal);
        self
    }

    /// Set the drag hover handler. Called when a drag payload hovers over this widget.
    pub fn on_drag_hover(
        mut self,
        f: impl FnMut(
            &crate::drag_payload::DragPayload,
            bastyde_canvas::Point,
            &mut EventContext,
        ) -> crate::drag_state::DropFeedback
        + 'static,
    ) -> Self {
        self.handler_set.handlers.on_drag_hover = Some(Box::new(f));
        self
    }

    /// Set the drag-leave handler. See [`HandlerSet::on_drag_leave`].
    pub fn on_drag_leave(mut self, f: impl FnMut(&mut EventContext) + 'static) -> Self {
        self.handler_set.handlers.on_drag_leave = Some(Box::new(f));
        self
    }

    /// Set the per-frame drag-tick handler. See [`HandlerSet::on_drag_tick`].
    pub fn on_drag_tick(
        mut self,
        f: impl FnMut(bastyde_canvas::Point, &mut EventContext) + 'static,
    ) -> Self {
        self.handler_set.handlers.on_drag_tick = Some(Box::new(f));
        self
    }

    /// Set the drop handler. Called when a payload is dropped on this widget.
    pub fn on_drop(
        mut self,
        f: impl FnMut(crate::drag_payload::DragPayload, bastyde_canvas::Point, &mut EventContext) -> bool
        + 'static,
    ) -> Self {
        self.handler_set.handlers.on_drop = Some(Box::new(f));
        self
    }

    // ── Accessibility overrides ────────────────────────────────────────
    //
    // All string-accepting methods take `impl Into<String>`. With the
    // `i18n` feature enabled, `bastyde_i18n::LocalizedString` (the type
    // produced by `tr!(...)`) provides `From<LocalizedString> for String`,
    // so `.access_label(tr!("save"))` works directly — the conversion
    // resolves the translation at builder time. The `_literal` twins
    // are `#[doc(hidden)]` grep markers for explicitly untranslated
    // call sites — same convention as `Button::new_literal`.

    /// Override the accessibility label (`Node::label`) of this widget.
    /// Replaces whatever the inner widget emitted via `set_name`.
    ///
    /// Accepts any `impl Into<String>`. With the `i18n` feature
    /// enabled, `bastyde_i18n::LocalizedString` (the type produced by
    /// `tr!(...)`) implements `From<LocalizedString> for String`, so
    /// `.access_label(tr!("save"))` works directly. Translation is
    /// resolved eagerly at builder time; the composite rebuild on
    /// locale change re-runs the chain to pick up new translations.
    pub fn access_label(mut self, label: impl Into<String>) -> Self {
        self.handler_set.access_mut().label = Some(label.into());
        self
    }

    /// `#[doc(hidden)]` grep marker for explicitly-untranslated label
    /// strings — the same convention as
    /// [`Button::new_literal`](crate::widget_builder::WidgetWithHandlers).
    /// Functionally identical to [`access_label`]; the distinct name
    /// makes untranslated call sites greppable as a one-pass audit.
    #[doc(hidden)]
    pub fn access_label_literal(self, label: impl Into<String>) -> Self {
        self.access_label(label)
    }

    /// Override the accessibility description (`Node::description`).
    /// Same conversion rules as [`access_label`].
    pub fn access_description(mut self, description: impl Into<String>) -> Self {
        self.handler_set.access_mut().description = Some(description.into());
        self
    }

    #[doc(hidden)]
    pub fn access_description_literal(self, description: impl Into<String>) -> Self {
        self.access_description(description)
    }

    /// Long-form context hint. Alias of [`access_description`] —
    /// AccessKit has no separate hint slot (SwiftUI's split is
    /// VoiceOver-specific). Provided for SwiftUI parity.
    pub fn access_hint(self, hint: impl Into<String>) -> Self {
        self.access_description(hint)
    }

    #[doc(hidden)]
    pub fn access_hint_literal(self, hint: impl Into<String>) -> Self {
        self.access_description(hint)
    }

    /// Override the accessibility value (`Node::value`).
    /// Same conversion rules as [`access_label`].
    pub fn access_value(mut self, value: impl Into<String>) -> Self {
        self.handler_set.access_mut().value = Some(value.into());
        self
    }

    #[doc(hidden)]
    pub fn access_value_literal(self, value: impl Into<String>) -> Self {
        self.access_value(value)
    }

    /// Override the accessibility role.
    pub fn access_role(mut self, role: accesskit::Role) -> Self {
        self.handler_set.access_mut().role = Some(role);
        self
    }

    /// Hide (or un-hide) this node from assistive technologies.
    /// `false` un-sets a hidden state the inner widget may have emitted
    /// unconditionally (e.g. `Panel::a11y_presentational`).
    pub fn access_hidden(mut self, hidden: bool) -> Self {
        self.handler_set.access_mut().hidden = Some(hidden);
        self
    }

    /// Mark (or un-mark) this widget as disabled for AT. `false`
    /// clears both widget-emitted disabled state AND the framework's
    /// arena-driven disabled gate at
    /// `accessibility_impl::build_accessibility_recursive`.
    pub fn access_disabled(mut self, disabled: bool) -> Self {
        self.handler_set.access_mut().disabled = Some(disabled);
        self
    }

    /// Stable test/debug identifier (`Node::author_id`). Not
    /// user-visible — used by accessibility inspectors and UI tests.
    pub fn access_identifier(mut self, id: impl Into<String>) -> Self {
        self.handler_set.access_mut().identifier = Some(id.into());
        self
    }

    /// Append a `controls` relationship. The target widget's NodeId
    /// is included in this node's `aria-controls`-equivalent list.
    pub fn access_controls(mut self, target: WidgetId) -> Self {
        self.handler_set.access_mut().controls.push(target);
        self
    }

    /// Append a `described_by` relationship.
    pub fn access_described_by(mut self, target: WidgetId) -> Self {
        self.handler_set.access_mut().described_by.push(target);
        self
    }

    /// Append a `labelled_by` relationship.
    pub fn access_labelled_by(mut self, target: WidgetId) -> Self {
        self.handler_set.access_mut().labelled_by.push(target);
        self
    }

    /// Set the live-region politeness (`Node::live`).
    pub fn access_live(mut self, mode: accesskit::Live) -> Self {
        self.handler_set.access_mut().live = Some(mode);
        self
    }

    /// Mark this node as the current item within its container
    /// (`aria-current`).
    pub fn access_current(mut self, current: accesskit::AriaCurrent) -> Self {
        self.handler_set.access_mut().aria_current = Some(current);
        self
    }

    /// Pre-formatted shortcut announcement (e.g. `"Ctrl+S"`). Used for
    /// chords NOT routed through the `Shortcut` system — platform-native
    /// keys, app-internal hotkeys not exposed to user rebinding. For
    /// `Shortcut`-registered chords prefer
    /// [`access_shortcut_id`](Self::access_shortcut_id), which tracks
    /// rebinds automatically.
    pub fn access_shortcut_literal(mut self, shortcut: impl Into<String>) -> Self {
        self.handler_set.access_mut().shortcut = Some(shortcut.into());
        self
    }

    /// Bind the announced shortcut to a registered `Shortcut` id (the
    /// same id you pass to `Shortcut::new("app.save")`). The
    /// accessibility tree walker resolves the current keystroke from
    /// `WidgetTree::shortcut_registry()` at AT-build time, formats it
    /// via `KeyStroke::Display` (`"Ctrl+S"`), and writes it to
    /// `Node::keyboard_shortcut`. Auto-refreshes on rebind.
    ///
    /// If the registry has no entry for `id` (no widget registered the
    /// shortcut yet), the announcement is omitted — same fallback as
    /// `MenuItem::for_shortcut(...)`.
    pub fn access_shortcut_id(mut self, id: impl Into<String>) -> Self {
        self.handler_set.access_mut().shortcut_id = Some(id.into());
        self
    }

    /// Indicate that activating this widget pops up a menu / listbox /
    /// dialog (`aria-haspopup`).
    pub fn access_has_popup(mut self, kind: accesskit::HasPopup) -> Self {
        self.handler_set.access_mut().has_popup = Some(kind);
        self
    }

    /// Override orientation (`Node::orientation`) — used on sliders,
    /// scrollbars, separators.
    pub fn access_orientation(mut self, orientation: accesskit::Orientation) -> Self {
        self.handler_set.access_mut().orientation = Some(orientation);
        self
    }

    /// Prune all descendants from the accessibility tree. The widget's
    /// own AT node is still emitted; only children disappear. Use for
    /// purely decorative composites. Flutter's `excludeSemantics: true`.
    pub fn access_exclude_subtree(mut self) -> Self {
        self.handler_set.access_subtree = Some(AccessSubtreeMode::Exclude);
        self
    }

    /// Lift descendants' labels / descriptions / values / actions into
    /// this widget's AT node, then prune the descendants. The whole
    /// composite reads as a single AT element. Flutter's
    /// `mergeAllDescendants: true` and SwiftUI's
    /// `.accessibilityElement(children: .combine)`.
    pub fn access_merge_subtree(mut self) -> Self {
        self.handler_set.access_subtree = Some(AccessSubtreeMode::Merge);
        self
    }

    /// Set an explicit subtree mode.
    pub fn access_subtree(mut self, mode: AccessSubtreeMode) -> Self {
        self.handler_set.access_subtree = Some(mode);
        self
    }

    /// Override `Node::numeric_value`.
    pub fn access_numeric_value(mut self, value: f64) -> Self {
        self.handler_set.access_mut().numeric_value = Some(value);
        self
    }

    /// Override `Node::min_numeric_value` and `max_numeric_value`.
    pub fn access_numeric_range(mut self, min: f64, max: f64) -> Self {
        let access = self.handler_set.access_mut();
        access.min_numeric_value = Some(min);
        access.max_numeric_value = Some(max);
        self
    }

    /// Override `Node::numeric_value_step`.
    pub fn access_numeric_step(mut self, step: f64) -> Self {
        self.handler_set.access_mut().numeric_step = Some(step);
        self
    }

    /// Advertise an accessibility action and the callback that fires
    /// when AT software invokes it. Multiple `access_action` calls
    /// register separate callbacks for distinct actions; calling twice
    /// with the same action records both — they fire in order.
    pub fn access_action<F>(mut self, action: accesskit::Action, handler: F) -> Self
    where
        F: FnMut(&mut EventContext) + 'static,
    {
        self.handler_set
            .access_mut()
            .actions
            .push((action, Box::new(handler)));
        self
    }

    /// Suppress an action the inner widget emitted (e.g. neutralize
    /// `Action::Click` on a Button used purely as a layout shim).
    /// Applied after the widget's `accessibility()` runs but before
    /// override-advertised actions, so a subsequent `access_action`
    /// for the same action re-advertises it with the override-installed
    /// callback.
    pub fn access_remove_action(mut self, action: accesskit::Action) -> Self {
        self.handler_set.access_mut().removed_actions.push(action);
        self
    }

    /// Advertise a custom-named action (SwiftUI parity:
    /// `.accessibilityAction(named:_:)`). The label is exposed
    /// verbatim by AT software (e.g. VoiceOver's Actions rotor).
    /// Accepts `tr!(...)` via the `LocalizedString -> String`
    /// conversion in `bastyde-i18n`.
    pub fn access_custom_action<F>(mut self, label: impl Into<String>, handler: F) -> Self
    where
        F: FnMut(&mut EventContext) + 'static,
    {
        self.handler_set
            .access_mut()
            .custom_actions
            .push((label.into(), Box::new(handler)));
        self
    }

    #[doc(hidden)]
    pub fn access_custom_action_literal<F>(self, label: impl Into<String>, handler: F) -> Self
    where
        F: FnMut(&mut EventContext) + 'static,
    {
        self.access_custom_action(label, handler)
    }

    /// Final escape hatch — invoked after all typed override setters,
    /// with full `&mut AccessNodeBuilder` access (including
    /// `inner_mut()`). Use for synthetic-child surgery (rich text
    /// paragraphs, text runs) or any AccessKit field the typed
    /// surface doesn't cover.
    pub fn access_customize<F>(mut self, f: F) -> Self
    where
        F: Fn(&mut crate::accessibility::AccessNodeBuilder) + 'static,
    {
        self.handler_set.access_mut().customize = Some(Box::new(f));
        self
    }
}

// Delegate all Widget trait methods to the inner widget.
impl<W: Widget> std::fmt::Debug for WidgetWithHandlers<W> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WidgetWithHandlers")
            .field("widget", &self.widget)
            .field("handler_set", &self.handler_set)
            .finish()
    }
}

impl<W: Widget + 'static> Widget for WidgetWithHandlers<W> {
    fn build(
        &mut self,
        ctx: &mut crate::build_context::BuildContext,
    ) -> Vec<crate::widget_id::WidgetId> {
        self.widget.build(ctx)
    }

    fn layout_response(
        &self,
        proposal: bastyde_canvas::SizeProposal,
        ctx: &crate::widget::LayoutContext,
    ) -> crate::widget::LayoutResponse {
        self.widget.layout_response(proposal, ctx)
    }

    fn place_children(
        &self,
        bounds: bastyde_canvas::Rect,
        proposal: bastyde_canvas::SizeProposal,
        children: &mut [crate::widget::WidgetPlacement],
        ctx: &crate::widget::LayoutContext,
    ) {
        self.widget.place_children(bounds, proposal, children, ctx)
    }

    fn paint(
        &self,
        bounds: bastyde_canvas::Rect,
        canvas: &mut bastyde_canvas::Canvas,
        ctx: &crate::widget::PaintContext,
    ) {
        self.widget.paint(bounds, canvas, ctx)
    }

    fn accessibility(&self, builder: &mut crate::accessibility::AccessNodeBuilder) {
        self.widget.accessibility(builder)
    }

    fn children(&self) -> Vec<crate::widget_id::WidgetId> {
        self.widget.children()
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        self.widget.as_any()
    }

    fn clips_children(&self) -> bool {
        self.handler_set
            .clips_children
            .unwrap_or_else(|| self.widget.clips_children())
    }

    fn take_handler_set(&mut self) -> Option<HandlerSet> {
        Some(self.take_handler_set())
    }
}

// ---------------------------------------------------------------------------
// WidgetBuilder trait — the entry point
// ---------------------------------------------------------------------------

/// Blanket trait providing attached handler methods for all Widget types.
/// The first builder method call wraps the widget in `WidgetWithHandlers`.
pub trait WidgetBuilder: Widget + Sized + 'static {
    fn on_tap(
        self,
        f: impl FnMut(&TapEvent, &mut EventContext) + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_tap(f)
    }

    fn on_double_tap(
        self,
        f: impl FnMut(&TapEvent, &mut EventContext) + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_double_tap(f)
    }

    fn on_triple_tap(
        self,
        f: impl FnMut(&TapEvent, &mut EventContext) + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_triple_tap(f)
    }

    fn on_long_press(
        self,
        f: impl FnMut(&TapEvent, &mut EventContext) + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_long_press(f)
    }

    /// Restrict (or extend) the set of pointer buttons that fire
    /// `on_tap`. Default is [`ButtonMask::PRIMARY`].
    fn accept_tap_buttons(self, mask: impl Into<ButtonMask>) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).accept_tap_buttons(mask)
    }

    /// Restrict (or extend) the set of pointer buttons that fire
    /// `on_double_tap`. Default [`ButtonMask::PRIMARY`].
    fn accept_double_tap_buttons(self, mask: impl Into<ButtonMask>) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).accept_double_tap_buttons(mask)
    }

    /// Restrict (or extend) the set of pointer buttons that fire
    /// `on_triple_tap`. Default [`ButtonMask::PRIMARY`].
    fn accept_triple_tap_buttons(self, mask: impl Into<ButtonMask>) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).accept_triple_tap_buttons(mask)
    }

    /// Restrict (or extend) the set of pointer buttons that fire
    /// `on_long_press`. Default [`ButtonMask::PRIMARY`].
    fn accept_long_press_buttons(self, mask: impl Into<ButtonMask>) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).accept_long_press_buttons(mask)
    }

    fn on_drag(
        self,
        f: impl FnMut(DragPhase, &mut EventContext) + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_drag(f)
    }

    fn on_swipe(
        self,
        f: impl FnMut(SwipeDirection, f32, &mut EventContext) + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_swipe(f)
    }

    fn on_pinch(
        self,
        f: impl FnMut(PinchPhase, &mut EventContext) + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_pinch(f)
    }

    fn on_focus(
        self,
        f: impl FnMut(bool, &mut EventContext) + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_focus(f)
    }

    fn on_key(
        self,
        f: impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_key(f)
    }

    /// Strict-ancestor key preview. See [`HandlerSet::on_key_preview`].
    fn on_key_preview(
        self,
        f: impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_key_preview(f)
    }

    fn on_pointer_event(
        self,
        f: impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_pointer_event(f)
    }

    fn on_hover(
        self,
        f: impl FnMut(bool, &mut EventContext) + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_hover(f)
    }

    fn on_scroll(
        self,
        f: impl FnMut(&WidgetEvent, &mut EventContext) -> EventResponse + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_scroll(f)
    }

    fn on_access_action(
        self,
        f: impl FnMut(accesskit::Action, &mut EventContext) -> EventResponse + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_access_action(f)
    }

    fn focusable(self, focusable: bool) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).focusable(focusable)
    }

    fn tab_index(self, index: i32) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).tab_index(index)
    }

    fn cursor(self, cursor: CursorIcon) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).cursor(cursor)
    }

    fn clips_children_on(self, clips: bool) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).clips_children(clips)
    }

    /// Opt this node in or out of OS IME composition while focused. See
    /// [`HandlerSet::ime_allowed`].
    fn ime_allowed(self, allowed: bool) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).ime_allowed(allowed)
    }

    /// Make the widget invisible to pointer hit-testing. See
    /// [`HandlerSet::event_pass_through`].
    fn event_pass_through(self, pass_through: bool) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).event_pass_through(pass_through)
    }

    /// Set a context-menu factory. See
    /// [`HandlerSet::context_menu`] for the full contract.
    fn context_menu(
        self,
        factory: impl Fn(Point, &mut EventContext) -> Option<Box<dyn Widget>> + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).context_menu(factory)
    }

    /// Bind a `Signal<bool>` the framework writes when a strict
    /// descendant has focus. See [`HandlerSet::focus_within`].
    fn focus_within(self, signal: crate::signal::Signal<bool>) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).focus_within(signal)
    }

    /// Bind a `Signal<bool>` the framework writes when a strict
    /// descendant is hovered. See [`HandlerSet::hover_within`].
    fn hover_within(self, signal: crate::signal::Signal<bool>) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).hover_within(signal)
    }

    fn on_drag_hover(
        self,
        f: impl FnMut(
            &crate::drag_payload::DragPayload,
            bastyde_canvas::Point,
            &mut EventContext,
        ) -> crate::drag_state::DropFeedback
        + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_drag_hover(f)
    }

    fn on_drag_leave(self, f: impl FnMut(&mut EventContext) + 'static) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_drag_leave(f)
    }

    fn on_drag_tick(
        self,
        f: impl FnMut(bastyde_canvas::Point, &mut EventContext) + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_drag_tick(f)
    }

    fn on_drop(
        self,
        f: impl FnMut(crate::drag_payload::DragPayload, bastyde_canvas::Point, &mut EventContext) -> bool
        + 'static,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).on_drop(f)
    }

    // ── Accessibility overrides ────────────────────────────────────────
    //
    // Trait-level entry points: each method wraps the widget into a
    // `WidgetWithHandlers` (the first builder call in any chain) and
    // forwards to the inherent method of the same name. See
    // `WidgetWithHandlers` for full rustdoc on each method's semantics.
    // For translated strings, `LocalizedString` flows through
    // `impl Into<String>` via `bastyde-i18n`'s `From<LocalizedString>` impl.

    fn access_label(self, label: impl Into<String>) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_label(label)
    }

    #[doc(hidden)]
    fn access_label_literal(self, label: impl Into<String>) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_label(label)
    }

    fn access_description(self, description: impl Into<String>) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_description(description)
    }

    #[doc(hidden)]
    fn access_description_literal(
        self,
        description: impl Into<String>,
    ) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_description(description)
    }

    fn access_hint(self, hint: impl Into<String>) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_hint(hint)
    }

    #[doc(hidden)]
    fn access_hint_literal(self, hint: impl Into<String>) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_hint(hint)
    }

    fn access_value(self, value: impl Into<String>) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_value(value)
    }

    #[doc(hidden)]
    fn access_value_literal(self, value: impl Into<String>) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_value(value)
    }

    fn access_role(self, role: accesskit::Role) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_role(role)
    }

    fn access_hidden(self, hidden: bool) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_hidden(hidden)
    }

    fn access_disabled(self, disabled: bool) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_disabled(disabled)
    }

    fn access_identifier(self, id: impl Into<String>) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_identifier(id)
    }

    fn access_controls(self, target: WidgetId) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_controls(target)
    }

    fn access_described_by(self, target: WidgetId) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_described_by(target)
    }

    fn access_labelled_by(self, target: WidgetId) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_labelled_by(target)
    }

    fn access_live(self, mode: accesskit::Live) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_live(mode)
    }

    fn access_current(self, current: accesskit::AriaCurrent) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_current(current)
    }

    fn access_shortcut_literal(self, shortcut: impl Into<String>) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_shortcut_literal(shortcut)
    }

    fn access_shortcut_id(self, id: impl Into<String>) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_shortcut_id(id)
    }

    fn access_has_popup(self, kind: accesskit::HasPopup) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_has_popup(kind)
    }

    fn access_orientation(self, orientation: accesskit::Orientation) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_orientation(orientation)
    }

    fn access_exclude_subtree(self) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_exclude_subtree()
    }

    fn access_merge_subtree(self) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_merge_subtree()
    }

    fn access_subtree(self, mode: AccessSubtreeMode) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_subtree(mode)
    }

    fn access_numeric_value(self, value: f64) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_numeric_value(value)
    }

    fn access_numeric_range(self, min: f64, max: f64) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_numeric_range(min, max)
    }

    fn access_numeric_step(self, step: f64) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_numeric_step(step)
    }

    fn access_action<F>(self, action: accesskit::Action, handler: F) -> WidgetWithHandlers<Self>
    where
        F: FnMut(&mut EventContext) + 'static,
    {
        WidgetWithHandlers::new(self).access_action(action, handler)
    }

    fn access_remove_action(self, action: accesskit::Action) -> WidgetWithHandlers<Self> {
        WidgetWithHandlers::new(self).access_remove_action(action)
    }

    fn access_custom_action<F>(
        self,
        label: impl Into<String>,
        handler: F,
    ) -> WidgetWithHandlers<Self>
    where
        F: FnMut(&mut EventContext) + 'static,
    {
        WidgetWithHandlers::new(self).access_custom_action(label, handler)
    }

    #[doc(hidden)]
    fn access_custom_action_literal<F>(
        self,
        label: impl Into<String>,
        handler: F,
    ) -> WidgetWithHandlers<Self>
    where
        F: FnMut(&mut EventContext) + 'static,
    {
        WidgetWithHandlers::new(self).access_custom_action(label, handler)
    }

    fn access_customize<F>(self, f: F) -> WidgetWithHandlers<Self>
    where
        F: Fn(&mut crate::accessibility::AccessNodeBuilder) + 'static,
    {
        WidgetWithHandlers::new(self).access_customize(f)
    }
}

// Blanket implementation for all Widget types.
impl<W: Widget + Sized + 'static> WidgetBuilder for W {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widget::WidgetPlacement;
    use crate::widget_id::WidgetId;
    use crate::widget_tree::WidgetTree;

    #[derive(Debug)]
    struct CompositeLeaf {
        child_id: Option<WidgetId>,
    }

    impl CompositeLeaf {
        fn new() -> Self {
            Self { child_id: None }
        }
    }

    impl Widget for CompositeLeaf {
        fn build(&mut self, ctx: &mut crate::build_context::BuildContext) -> Vec<WidgetId> {
            let child = ctx.add(crate::test_widgets::FillWidget::new());
            self.child_id = Some(child);
            vec![child]
        }

        fn layout_response(
            &self,
            proposal: bastyde_canvas::SizeProposal,
            _ctx: &crate::widget::LayoutContext,
        ) -> crate::widget::LayoutResponse {
            proposal.resolve(120.0, 40.0).into()
        }

        fn place_children(
            &self,
            bounds: bastyde_canvas::Rect,
            _proposal: bastyde_canvas::SizeProposal,
            children: &mut [WidgetPlacement],
            _ctx: &crate::widget::LayoutContext,
        ) {
            for child in children.iter_mut() {
                child.origin = bounds.origin();
                child.size = bounds.size();
            }
        }

        fn children(&self) -> Vec<WidgetId> {
            self.child_id.into_iter().collect()
        }
    }

    #[test]
    fn external_handlers_survive_rebuild() {
        // Regression check: handlers attached externally via the
        // `WidgetBuilder` builder (e.g. `MyCompositeWidget::new().on_tap(...)`)
        // must continue to fire after the widget rebuilds in place.
        // My handler-clearing fix in `rebuild_single_widget` wiped
        // `node.handlers` to stop accumulation of `apply_self_handlers`
        // calls across rebuilds — but the extracted-once-at-insertion
        // HandlerSet is gone by rebuild time and would be lost.
        use std::cell::Cell;
        use std::rc::Rc;

        let tap_count = Rc::new(Cell::new(0_u32));
        let tc = tap_count.clone();

        let mut tree = WidgetTree::new();
        let id = tree.add(CompositeLeaf::new().on_tap(move |_pos, _ctx| {
            tc.set(tc.get() + 1);
        }));
        tree.layout(bastyde_canvas::SizeProposal::exact(200.0, 100.0));

        // Trip a rebuild of the composite — its child gets torn down &
        // rebuilt; node.handlers gets cleared and reset.
        tree.arena_mark_needs_rebuild_for_testing(id);
        tree.layout(bastyde_canvas::SizeProposal::exact(200.0, 100.0));

        // Click through the composite; the externally-attached on_tap
        // must still be wired up.
        tree.click(id);
        assert_eq!(
            tap_count.get(),
            1,
            "externally-attached on_tap must survive a rebuild"
        );
    }

    #[test]
    fn wrapped_composite_widget_still_builds_children() {
        let mut tree = WidgetTree::new();
        let root = tree.add(CompositeLeaf::new().on_tap(|_pos, _ctx| {}));
        tree.layout(bastyde_canvas::SizeProposal::exact(200.0, 100.0));

        assert_eq!(tree.children(root).len(), 1);
    }
}
