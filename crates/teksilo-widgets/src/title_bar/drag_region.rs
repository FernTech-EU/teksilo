// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! `DragRegion` — flexible drag region inside a `TitleBar`.
//!
//! Captures pointer events that are not consumed by inner content and
//! forwards them to the platform host: drag gestures begin a window move,
//! double taps toggle maximize, and right-clicks open the system window
//! menu (Wayland only). On Windows the drag rect is published into
//! `HitRegions::drag` so the wndproc subclass returns `HTCAPTION` for
//! the same area — but the actual publish happens from
//! [`crate::title_bar::TitleBar::after_paint`], which aggregates this
//! drag region and the three control buttons into one snapshot per
//! frame. This widget no longer publishes from `paint()`.
//!
//! The region grows via `flex = 1.0` to claim all remaining horizontal
//! space in the parent `HStack`, so it naturally sits between any leading
//! widgets (app icon, document title) and the trailing `WindowControls`
//! cluster. An optional child widget — typically a centered title — is
//! placed at the full region bounds and passes pointer events upward to
//! the drag handler when it does not consume them.
//!
//! # A finger on the title bar
//!
//! The drag region is deliberately **not** a press-time actor: the window move
//! starts from [`DragPhase::Started`], after the recognizer has decided the
//! press is a drag, which is why a quick press still reaches the double-tap
//! recognizer and why a contact that turns out to be a scroll is never stolen.
//! Nothing here changes that.
//!
//! Three routes serve a contact:
//!
//! * **Double tap → maximise / restore.** The same handler a double click
//!   drives; the multi-tap recognizer already tunes its slop per pointer kind,
//!   so a finger's looser aim is accounted for without a second code path.
//! * **Long press → the window menu.** A mouse reaches it with the secondary
//!   button, which a finger does not have. Where the platform owns the menu
//!   (Wayland's `xdg_toplevel.show_window_menu`) the long press asks the host
//!   for it, and that menu's **Move** entry is a finger's only route to a
//!   window move — see below. Where it does not (X11), this widget's
//!   `.context_menu(..)` factory is the menu, and it is reached through the
//!   framework's own long-press context-menu route rather than through a second
//!   copy of the opening machinery here.
//! * **Drag → nothing, on purpose.** `BackendCaps::touch_window_drag` is
//!   `false` on every platform Teksilo supports, and it is not a to-do:
//!   `xdg_toplevel::move` needs a serial from an input event on a toplevel the
//!   compositor agrees the client owns, and winit 0.30's `drag_window` harvests
//!   a *pointer* serial internally, so a finger cannot reach it however the app
//!   asks. Calling it anyway would be a silent no-op — the compositor drops a
//!   request whose serial does not match — so the drag handler does not call
//!   it for a direct pointer, and the long-press menu is the documented
//!   alternative. This is also the WCAG 2.5.7 single-pointer alternative to the
//!   dragging operation. A headless test cannot detect the real failure (a fake
//!   `PlatformTitleBarHost` will happily record a `begin_drag` that a
//!   compositor would have ignored), so this is a hardware-checklist line.
//!
//! ```ignore
//! // Used internally by TitleBar; the snippet shows the construction pattern.
//! let region = DragRegion::with_child(host.clone(), TextWidget::new(lit!("My App")));
//! ```

use std::rc::Rc;

use teksilo_canvas::{Rect, Size, SizeProposal};
use teksilo_core::PlatformTitleBarHost;
use teksilo_core::accessibility::AccessNodeBuilder;
use teksilo_core::event::{EventResponse, PointerButton, WidgetEvent};
use teksilo_core::gesture::DragPhase;
use teksilo_core::widget::{LayoutContext, PaintContext, PendingChild, Widget, WidgetPlacement};
use teksilo_core::widget_builder::HandlerSet;
use teksilo_core::widget_id::WidgetId;

/// Flexible, hit-transparent region inside a title bar that routes pointer events to the
/// platform host for window dragging, maximize-toggle, and the system window menu.
pub struct DragRegion {
    host: Rc<dyn PlatformTitleBarHost>,
    pending_child: Option<PendingChild>,
    child_id: Option<WidgetId>,
    /// Forwarded from [`TitleBar::close_action`](crate::TitleBar::close_action)
    /// so the fallback menu's Close entry does exactly what the close *button*
    /// does. Unused when the platform has its own window menu.
    close_action: Option<Rc<dyn Fn(&mut teksilo_core::widget::EventContext)>>,
}

impl std::fmt::Debug for DragRegion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragRegion")
            .field("has_child", &self.pending_child.is_some())
            .finish_non_exhaustive()
    }
}

impl DragRegion {
    /// Create a drag region with no inner content — the entire region is a pure drag handle.
    pub fn new(host: Rc<dyn PlatformTitleBarHost>) -> Self {
        Self {
            host,
            pending_child: None,
            child_id: None,
            close_action: None,
        }
    }

    /// Create a drag region wrapping an arbitrary boxed child widget (typically a centered
    /// title). Pointer events not consumed by the child bubble up to the drag handler.
    pub fn with_child(host: Rc<dyn PlatformTitleBarHost>, child: Box<dyn Widget>) -> Self {
        Self {
            host,
            pending_child: Some(PendingChild::Deferred(child)),
            child_id: None,
            close_action: None,
        }
    }

    /// Create a drag region with an already-registered child identified by `id`.
    /// Use this when the child widget was added to the tree before constructing the
    /// region (e.g. when you need the child's `WidgetId` for another reference).
    pub fn with_child_id(host: Rc<dyn PlatformTitleBarHost>, id: WidgetId) -> Self {
        Self {
            host,
            pending_child: Some(PendingChild::Id(id)),
            child_id: None,
            close_action: None,
        }
    }

    /// Forward the title bar's close-action override, so the fallback window
    /// menu's Close entry matches the close button. No effect on platforms
    /// that provide their own window menu.
    pub fn close_action(
        mut self,
        action: Option<Rc<dyn Fn(&mut teksilo_core::widget::EventContext)>>,
    ) -> Self {
        self.close_action = action;
        self
    }
}

impl Widget for DragRegion {
    fn build(&mut self, ctx: &mut teksilo_core::build_context::BuildContext) -> Vec<WidgetId> {
        if let Some(pending) = self.pending_child.take() {
            self.child_id = Some(match pending {
                PendingChild::Id(id) => id,
                PendingChild::Deferred(w) => ctx.add_boxed(w),
            });
        }

        // Drag gesture: begin a window move as soon as the OS recognizes
        // movement during a primary-button press. Using `on_drag` (rather
        // than `on_pointer_event` on PointerDown) means a quick click
        // without movement still flows to the double-tap recognizer, which
        // is how we get double-click-to-maximize.
        let host_drag = self.host.clone();
        let host_pointer = self.host.clone();

        // Right-click opens the window menu. Where the OS provides one we ask
        // for it; where it does not (X11 — see `window_menu`), we build our
        // own via the framework's context-menu factory, which handles the
        // at-pointer overlay, dismissal, and focus for us.
        let has_os_window_menu = self.host.has_window_menu();
        let close_action = self.close_action.clone();

        // The window menu a long press asks for, where the platform has one.
        // A mouse reaches the same menu with the secondary button below.
        let host_long_press = self.host.clone();

        let mut handlers = HandlerSet::new()
            .on_drag(move |phase, _ctx| {
                if let DragPhase::Started {
                    button: PointerButton::Primary,
                    pointer,
                    ..
                } = phase
                {
                    // A direct pointer cannot start an OS window move under
                    // winit 0.30 on any backend — `drag_window` harvests a
                    // pointer serial, and every `BackendCaps::touch_window_drag`
                    // is `false`. Asking would be a silent no-op, so the finger
                    // is routed to the long-press window menu (whose Move entry
                    // is the non-drag alternative) instead of being told a lie.
                    if pointer.kind.is_direct() {
                        return;
                    }
                    let _ = host_drag.begin_drag();
                }
            })
            .on_long_press(move |event, ctx| {
                // The secondary button is a mouse's route to this menu; a
                // finger has no second button, so a hold is its equivalent.
                // Precise pointers are excluded so a held mouse button, which
                // is the *start of a drag*, never pops a menu mid-move.
                if !event.pointer.kind.is_direct() || !has_os_window_menu {
                    return;
                }
                // Window-logical, not widget-local: `show_window_menu` is
                // documented in client-area coordinates and the drag region
                // starts wherever the title bar's leading content ends.
                let at = ctx.pointer_position().unwrap_or(event.position);
                let _ = host_long_press.show_window_menu(at);
            })
            .on_double_tap(move |_pos, ctx| {
                if let Some(w) = ctx.window() {
                    let next = if w.placement().get().is_maximized() {
                        teksilo_core::WindowPlacement::Floating
                    } else {
                        teksilo_core::WindowPlacement::Maximized
                    };
                    w.placement().set(next);
                }
            })
            .on_pointer_event(move |evt, _ctx| {
                if !has_os_window_menu {
                    // The context-menu factory below owns the secondary
                    // button; consuming it here would suppress the menu.
                    return EventResponse::Ignored;
                }
                if let WidgetEvent::PointerDown {
                    button: PointerButton::Secondary,
                    position,
                    ..
                } = evt
                {
                    let _ = host_pointer.show_window_menu(*position);
                    return EventResponse::Handled;
                }
                EventResponse::Ignored
            });

        if !has_os_window_menu {
            handlers = handlers.context_menu(move |_at, ctx| {
                super::window_menu::build_window_menu(ctx, close_action.clone())
            });
        }

        ctx.apply_self_handlers(handlers);

        self.child_id.into_iter().collect()
    }

    fn layout_response(
        &self,
        proposal: SizeProposal,
        _ctx: &LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        // Wanted width is 0 — we want pure slack from the parent HStack.
        // Height matches the title bar's configured height so we paint
        // through even when the inner child reports zero.
        // `flex = 1.0` claims the leftover horizontal space; without it the
        // drag region collapses and there is nothing to drag.
        teksilo_core::widget::LayoutResponse::flexible(
            Size::new(0.0, proposal.height.unwrap_or(0.0)),
            1.0,
        )
    }

    fn place_children(
        &self,
        bounds: Rect,
        _proposal: SizeProposal,
        children: &mut [WidgetPlacement],
        _ctx: &LayoutContext,
    ) {
        // Inner content (the optional `center` widget) fills the drag
        // region's full bounds.
        for child in children.iter_mut() {
            child.origin = bounds.origin();
            child.size = bounds.size();
        }
    }

    fn paint(&self, _bounds: Rect, _canvas: &mut teksilo_canvas::Canvas, _ctx: &PaintContext) {
        // No paint — our parent `TitleBar::after_paint` reads our
        // bounds and publishes them as part of the aggregated
        // `HitRegions` snapshot.
    }

    fn accessibility(&self, builder: &mut AccessNodeBuilder) {
        // Pointer-only affordance: no keyboard or AT analogue for "drag the
        // window by its title". A bare `GenericContainer` keeps the region
        // from showing up as an unnamed stop between the title bar landmark
        // and its real content, since every adapter drops it and promotes
        // what it holds. Never `set_hidden()`: the region holds
        // `TitleBar::center`, a search box or breadcrumbs, and an adapter
        // reads a hidden node as hiding all of that with it.
        builder.set_role(teksilo_core::accesskit::Role::GenericContainer);
    }

    fn children(&self) -> Vec<WidgetId> {
        self.child_id.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::time::Duration;

    use teksilo_canvas::{Point, Size, SizeProposal};
    use teksilo_core::event::{Modifiers, PointerButton};
    use teksilo_core::pointer::clock::ManualClock;
    use teksilo_core::pointer::{
        BackendDeviceKey, EventTime, PointerIdAllocator, PointerInfo, PointerPhase, PointerSample,
    };
    use teksilo_core::widget::LayoutResponse;
    use teksilo_core::widget_tree::WidgetTree;
    use teksilo_core::window::WindowState;
    use teksilo_core::{
        HitRegions, PlatformError, ResizeEdge, TeksiloWindowId, WindowPlacement, WindowStateInit,
    };

    use super::*;

    #[derive(Default)]
    struct TestHost {
        drags: Cell<u32>,
        menus: Cell<u32>,
        os_menu: bool,
    }

    impl TestHost {
        fn with_os_menu() -> Self {
            Self {
                os_menu: true,
                ..Self::default()
            }
        }
    }

    impl PlatformTitleBarHost for TestHost {
        fn reserved_leading_inset(&self) -> Size {
            Size::ZERO
        }
        fn reserved_trailing_inset(&self) -> Size {
            Size::ZERO
        }
        fn renders_custom_controls(&self) -> bool {
            true
        }
        fn needs_custom_resize_handles(&self) -> bool {
            true
        }
        fn begin_drag(&self) -> Result<(), PlatformError> {
            self.drags.set(self.drags.get() + 1);
            Ok(())
        }
        fn begin_resize(&self, _edge: ResizeEdge) -> Result<(), PlatformError> {
            Ok(())
        }
        fn show_window_menu(&self, _at: Point) -> Result<(), PlatformError> {
            self.menus.set(self.menus.get() + 1);
            Ok(())
        }
        fn has_window_menu(&self) -> bool {
            self.os_menu
        }
        fn update_hit_regions(&self, _regions: &HitRegions) {}
    }

    /// The drag region alone in a 400 × 32 tree, on a manual input clock so a
    /// hold is a `set` rather than a sleep.
    fn region(host: Rc<TestHost>) -> (WidgetTree, Rc<ManualClock>, Point) {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let clock = Rc::new(ManualClock::new(EventTime::ZERO));
        tree.set_input_clock(clock.clone());
        tree.set_window_state(WindowState::new(WindowStateInit {
            id: TeksiloWindowId::new(1),
            string_id: Some("w1".to_string()),
            placement: WindowPlacement::Floating,
            title: "Test".to_string(),
            size: (400, 32),
            position: (0, 0),
            focused: true,
            resizable: true,
            always_on_top: false,
        }));
        let id = tree.add(DragRegion::new(host as Rc<dyn PlatformTitleBarHost>));
        tree.layout(SizeProposal::exact(400.0, 32.0));
        let at = tree.bounds(id).center();
        (tree, clock, at)
    }

    fn finger(raw: u64) -> PointerInfo {
        let id = PointerIdAllocator::global().begin(BackendDeviceKey::new(0x50D6), raw);
        let mut info = PointerInfo::touch(id, EventTime::ZERO);
        info.primary = true;
        info
    }

    fn contact(mut pointer: PointerInfo, phase: PointerPhase, at: Point, ms: u64) -> PointerSample {
        pointer.time = EventTime::from_millis(ms);
        PointerSample {
            pointer,
            phase,
            position: at,
            button: None,
            modifiers: Modifiers::NONE,
            coalesced: Vec::new(),
        }
    }

    /// A mouse drag on the title bar asks the platform for a window move, as it
    /// always has.
    #[test]
    fn a_mouse_drag_starts_a_window_move() {
        let host = Rc::new(TestHost::with_os_menu());
        let (mut tree, _clock, at) = region(host.clone());
        tree.pointer_move(at);
        tree.pointer_down_button(at, PointerButton::Primary);
        tree.pointer_move(Point::new(at.x + 40.0, at.y));
        assert_eq!(host.drags.get(), 1, "a mouse drag moves the window");
    }

    /// A finger drag does **not**, and that is a platform fact rather than an
    /// omission: `BackendCaps::touch_window_drag` is `false` everywhere because
    /// winit 0.30's `drag_window` harvests a pointer serial, so the request
    /// would be dropped by the compositor. The finger's route is the long-press
    /// window menu below.
    #[test]
    fn a_finger_drag_does_not_ask_for_a_window_move() {
        let host = Rc::new(TestHost::with_os_menu());
        let (mut tree, _clock, at) = region(host.clone());
        let f = finger(41);
        tree.dispatch_pointer(contact(f, PointerPhase::Down, at, 0));
        // Well past any drag slop, on both axes.
        for step in [30.0_f32, 60.0, 120.0] {
            tree.dispatch_pointer(contact(
                f,
                PointerPhase::Move,
                Point::new(at.x + step, at.y),
                10 + step as u64,
            ));
        }
        assert_eq!(
            host.drags.get(),
            0,
            "a finger must not ask for a move the compositor will drop"
        );
    }

    /// A hold with a finger opens the system window menu — the equivalent of
    /// the secondary button a finger does not have, and the route to its Move
    /// entry.
    #[test]
    fn a_long_press_opens_the_system_window_menu() {
        let host = Rc::new(TestHost::with_os_menu());
        let (mut tree, clock, at) = region(host.clone());
        let f = finger(42);
        tree.dispatch_pointer(contact(f, PointerPhase::Down, at, 0));

        clock.set(EventTime::from_millis(499));
        tree.tick_gestures(std::time::Instant::now());
        assert_eq!(host.menus.get(), 0, "499 ms is under the hold");

        clock.set(EventTime::from_millis(500));
        tree.tick_gestures(std::time::Instant::now() + Duration::from_millis(500));
        assert_eq!(host.menus.get(), 1, "500 ms is the hold");
    }

    /// A held **mouse** button does not: that is the start of a window move,
    /// and the mouse already reaches the menu with its secondary button.
    #[test]
    fn a_held_mouse_button_does_not_open_the_window_menu() {
        let host = Rc::new(TestHost::with_os_menu());
        let (mut tree, clock, at) = region(host.clone());
        tree.pointer_move(at);
        tree.pointer_down_button(at, PointerButton::Primary);
        clock.set(EventTime::from_millis(900));
        tree.tick_gestures(std::time::Instant::now() + Duration::from_millis(900));
        assert_eq!(host.menus.get(), 0);
    }

    /// The secondary button still opens it, unchanged.
    #[test]
    fn a_secondary_press_still_opens_the_system_window_menu() {
        let host = Rc::new(TestHost::with_os_menu());
        let (mut tree, _clock, at) = region(host.clone());
        tree.pointer_move(at);
        tree.pointer_down_button(at, PointerButton::Secondary);
        assert_eq!(host.menus.get(), 1);
    }

    /// Where the platform has no window menu of its own (X11), the long press
    /// asks for nothing — the fallback menu is this widget's `.context_menu(..)`
    /// factory, opened through the framework's own route rather than through a
    /// second copy of the opening machinery here.
    #[test]
    fn without_an_os_menu_the_long_press_asks_for_nothing() {
        let host = Rc::new(TestHost::default());
        let (mut tree, clock, at) = region(host.clone());
        let f = finger(43);
        tree.dispatch_pointer(contact(f, PointerPhase::Down, at, 0));
        clock.set(EventTime::from_millis(600));
        tree.tick_gestures(std::time::Instant::now() + Duration::from_millis(600));
        assert_eq!(host.menus.get(), 0);
    }

    /// Double tap toggles maximise, for a finger as for a mouse. The recognizer
    /// tunes its slop per pointer kind, so the two share one handler.
    #[test]
    fn a_double_tap_toggles_maximise() {
        for coarse in [false, true] {
            let host = Rc::new(TestHost::with_os_menu());
            let (mut tree, _clock, at) = region(host.clone());
            let placement = tree.window_state().unwrap().placement().clone();
            assert!(!placement.get().is_maximized());

            if coarse {
                let f = finger(44);
                for tap in 0..2u64 {
                    tree.dispatch_pointer(contact(f, PointerPhase::Down, at, tap * 100));
                    tree.dispatch_pointer(contact(f, PointerPhase::Up, at, tap * 100 + 20));
                }
            } else {
                tree.pointer_move(at);
                for _ in 0..2 {
                    tree.pointer_down_button(at, PointerButton::Primary);
                    tree.pointer_up_button(at, PointerButton::Primary);
                }
            }
            assert!(
                placement.get().is_maximized(),
                "a double tap must maximise (coarse = {coarse})"
            );
        }
    }

    /// The layout contract the drag region rests on: pure slack, no wanted
    /// width. Pinned because the grab work above must not have moved it.
    #[test]
    fn the_region_claims_only_slack() {
        let host: Rc<dyn PlatformTitleBarHost> = Rc::new(TestHost::default());
        let region = DragRegion::new(host);
        let theme = teksilo_core::presets::intui::light();
        let ctx = teksilo_core::widget::LayoutContext::for_testing(&theme);
        let response: LayoutResponse =
            region.layout_response(SizeProposal::exact(400.0, 32.0), &ctx);
        assert_eq!(response.size.width, 0.0);
        assert_eq!(response.flex, 1.0);
    }
}
