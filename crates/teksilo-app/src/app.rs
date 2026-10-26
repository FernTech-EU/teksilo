// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};
use teksilo_canvas::SizeProposal;
use teksilo_core::Theme;
use teksilo_core::app_event::AppEvent;
use teksilo_core::event::WidgetEvent;
use teksilo_core::event_source::{
    AppEventPoster, EventSource, EventSourceAdapter, SubscriptionId, TreeAppContext,
};
use teksilo_core::modal::{ModalCloseBehavior, ModalContent, ModalPresentation, ModalRequest};
use teksilo_core::{DismissBehavior, OverlayPlacement, OverlayRequest};
use teksilo_core::{WidgetId, WidgetTree};
use teksilo_i18n::{I18nConfig, I18nManager, LanguageIdentifier};
use teksilo_platform::event_translation;
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
#[allow(unused_imports)]
use winit::raw_window_handle::HasWindowHandle;
use winit::window::WindowId;

/// How the application resolves its theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    /// Use a specific fixed theme (current behavior, default).
    #[default]
    Manual,
    /// Follow the OS light/dark preference using Teksilo's built-in themes.
    FollowSystem,
    /// Adopt colors read directly from the OS/DE config files (GNOME/KDE/Cinnamon).
    /// Falls back to `FollowSystem` on unsupported platforms or DEs.
    Native,
}

/// When the application quits on its own. Set with
/// [`TeksiloAppBuilder::quit_policy`].
///
/// Whatever the policy, a quit that completes ends the application: the
/// guarded [`EventContext::quit`](teksilo_core::widget::EventContext::quit)
/// (what a Quit row and the macOS App menu's Quit ask for) once the
/// application's quit guard
/// ([`TeksiloAppBuilder::on_quit_requested`]) and every window's close guard
/// have agreed and every window has closed, any of them able to refuse or to
/// defer its answer, or the forced
/// [`EventContext::quit_forced`](teksilo_core::widget::EventContext::quit_forced).
/// The policy decides only what closing the last window does by itself.
///
/// [`TeksiloAppBuilder::initial_window`] is optional under either policy:
/// the application tree may open the first window from
/// [`run_after_mount`](teksilo_core::BuildContext::run_after_mount). Under
/// `LastWindowClosed`, an application with no window open once it has
/// started (the initial window created and the application tree's mount
/// actions run) panics on its first turn, since it would quit before
/// showing anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum QuitPolicy {
    /// The application quits when its last window closes. The default, and
    /// the only behaviour before this setting existed.
    #[default]
    LastWindowClosed,
    /// Closing the last window does not quit: the application keeps running
    /// with no window open, the way a background service or a tray
    /// application does, until it asks to with
    /// [`EventContext::quit`](teksilo_core::widget::EventContext::quit)
    /// or [`EventContext::quit_forced`](teksilo_core::widget::EventContext::quit_forced).
    /// It may also run with no window at all from the start.
    Explicit,
}

impl QuitPolicy {
    /// Whether the event loop should end, with `open_windows` windows open
    /// and `quit_completed` telling whether a quit request ran to its end.
    fn exits_with(self, open_windows: usize, quit_completed: bool) -> bool {
        quit_completed
            || match self {
                QuitPolicy::LastWindowClosed => open_windows == 0,
                QuitPolicy::Explicit => false,
            }
    }

    /// Why an application with this policy cannot go on with
    /// `open_windows` windows open once it has started (its initial window
    /// created and the application tree's mount actions run), `None` when
    /// it can.
    fn refuses_startup_without_window(self, open_windows: usize) -> Option<&'static str> {
        match self {
            QuitPolicy::LastWindowClosed if open_windows == 0 => Some(
                "TeksiloAppBuilder: no window is open once the application has started \
                 (no initial_window, and the application tree's run_after_mount opened \
                 none), and the quit policy is QuitPolicy::LastWindowClosed, so the \
                 application would quit before showing anything. Give it an \
                 initial_window, open one from the application tree's run_after_mount, \
                 or set quit_policy(QuitPolicy::Explicit) to run with no window.",
            ),
            QuitPolicy::LastWindowClosed | QuitPolicy::Explicit => None,
        }
    }
}

/// Builds the application tree's root: see [`TeksiloAppBuilder::app_root`].
pub type AppRootBuilder = Box<dyn FnOnce(&mut WidgetTree) -> teksilo_core::WidgetId>;

/// The application tree's root when the application gives none: a tree
/// with no root drops every intent sent from an application callback, so
/// there is always one.
#[derive(Debug)]
pub(crate) struct AppTreeAnchor;

impl teksilo_core::widget::Widget for AppTreeAnchor {
    fn layout_response(
        &self,
        proposal: teksilo_canvas::SizeProposal,
        _ctx: &teksilo_core::widget::LayoutContext,
    ) -> teksilo_core::widget::LayoutResponse {
        proposal.resolve(0.0, 0.0).into()
    }
}

/// The logic half of a frame, which a window's tree and the application
/// tree share: idle callbacks within `idle_budget`, then layout at
/// `proposal`, which runs the clocks, the reactive bindings (rebuilds,
/// effects) and the frame tick. Returns whether idle callbacks ran.
///
/// The application tree is laid out at a zero proposal: it has no window to
/// be sized to, and nothing in it is drawn.
pub(crate) fn run_tree_logic_frame(
    tree: &mut WidgetTree,
    idle_budget: Duration,
    proposal: SizeProposal,
    ops: &mut dyn teksilo_core::WindowOps,
) -> bool {
    let ran_idle = tree.has_idle_work();
    if ran_idle {
        tree.run_idle_callbacks(idle_budget);
    }
    tree.layout_with_ops(proposal, ops);
    ran_idle
}

/// Posted by the application tree's [`RedrawWaker`](teksilo_canvas::wake::RedrawWaker),
/// from any thread, when content off the UI thread asks that tree for a
/// frame. Private to the framework, like [`WindowWake`]; the loop answers
/// with a tick of the application tree. At most one is in flight.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AppTreeWake;

/// Posted when a deferred close is answered
/// ([`CloseDecision`](teksilo_core::CloseDecision)), from whatever thread
/// answers it. Private to the framework; it only wakes the loop, whose
/// `about_to_wait` then settles the answer.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CloseAnswered;

/// The application tree's waker: posts one [`AppTreeWake`] until the loop
/// has taken it, whatever the [`WakeKind`](teksilo_canvas::wake::WakeKind),
/// since every frame of that tree is a logic frame.
pub(crate) struct AppTreeWaker {
    pub(crate) post: std::sync::Arc<dyn Fn(Box<dyn std::any::Any + Send>) + Send + Sync>,
    pub(crate) pending: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl teksilo_canvas::wake::RedrawWaker for AppTreeWaker {
    fn wake(&self, _kind: teksilo_canvas::wake::WakeKind) {
        if !self.pending.swap(true, std::sync::atomic::Ordering::AcqRel) {
            (self.post)(Box::new(AppTreeWake));
        }
    }
}

use teksilo_text::SharedTypesetter;

use crate::window_config::{SizeToContent, TeksiloWindowId, WindowConfig};
use crate::window_manager::WindowManager;
use teksilo_core::WindowPlacement;

/// Interrogate the winit window for its current placement so an
/// `OS-initiated` state change can be mirrored into the corresponding
/// `WindowState::placement` signal without the observer pushing it
/// back out as a `WindowCommand` (re-entrancy guard on `from_os`).
fn query_window_placement(win: &winit::window::Window) -> WindowPlacement {
    if win.is_minimized() == Some(true) {
        WindowPlacement::Minimized
    } else if win.fullscreen().is_some() {
        WindowPlacement::Fullscreen
    } else if win.is_maximized() {
        WindowPlacement::Maximized
    } else {
        WindowPlacement::Floating
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolvedModalPresentation {
    InTree,
    NativeWindow,
}

/// Generate a per-process random session id for telemetry.
///
/// Not persisted across restarts — by design (a stable id would be
/// pseudonymous tracking, distinct from `InstallId`'s 13-month UUID).
/// The first 16 hex chars of a fresh UUID are sufficient for grouping
/// events within one process lifetime.
#[cfg(feature = "telemetry")]
fn generate_session_id() -> String {
    let uuid = uuid::Uuid::new_v4().simple().to_string();
    uuid[..16].to_string()
}

/// Whether a modal request with deferred content can be presented as a
/// native window of its own here
/// ([`supports_native_modal_windows`](teksilo_platform::supports_native_modal_windows)).
/// A test build can answer yes on X11, to run that path there
/// (`native_modal_override`).
fn native_modal_windows_supported() -> bool {
    #[cfg(test)]
    if let Some(supported) = native_modal_override::get() {
        return supported;
    }
    teksilo_platform::supports_native_modal_windows()
}

/// What a test build answers for [`native_modal_windows_supported`] on the
/// thread that set it, in place of the platform's answer.
#[cfg(test)]
pub(crate) mod native_modal_override {
    use std::cell::Cell;

    thread_local! {
        static OVERRIDE: Cell<Option<bool>> = const { Cell::new(None) };
    }

    /// Answer `supported` from now on; `None` gives the platform's answer
    /// back.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn set(supported: Option<bool>) {
        OVERRIDE.with(|cell| cell.set(supported));
    }

    pub(super) fn get() -> Option<bool> {
        OVERRIDE.with(Cell::get)
    }
}

fn resolve_modal_presentation(
    requested: ModalPresentation,
    content: &ModalContent,
    native_supported: bool,
) -> ResolvedModalPresentation {
    let can_use_native = native_supported && matches!(content, ModalContent::Deferred(_));

    match requested {
        ModalPresentation::InTree => ResolvedModalPresentation::InTree,
        ModalPresentation::NativeWindow => {
            if can_use_native {
                ResolvedModalPresentation::NativeWindow
            } else {
                ResolvedModalPresentation::InTree
            }
        }
        ModalPresentation::Auto => {
            if can_use_native {
                ResolvedModalPresentation::NativeWindow
            } else {
                ResolvedModalPresentation::InTree
            }
        }
    }
}

/// The size, in logical pixels, a window sizing itself to its content
/// ([`SizeToContent`]) wants at its `current` size: its content's height at
/// its width, the width kept. With `width_floor`, the width follows the
/// content too, never narrower than the floor: a native dialog window,
/// whose requested size is the floor and whose content (a dialog panel's
/// padding around a `MessageBox` of the requested width) can be wider.
/// `None` when the tree has nothing to measure.
fn size_to_content_target(
    tree: &WidgetTree,
    current: (f32, f32),
    width_floor: Option<u32>,
) -> Option<(u32, u32)> {
    let mut width = current.0.round() as u32;
    if let Some(floor) = width_floor {
        // What the content takes at the window's width: a rigid column
        // reports its own width, padding included, past the proposal.
        let wanted = tree.measure_root_intrinsic(SizeProposal {
            width: Some(current.0),
            height: None,
        })?;
        width = (wanted.width.ceil().max(1.0) as u32).max(floor);
    }
    let intrinsic = tree.measure_root_intrinsic(SizeProposal {
        width: Some(width as f32),
        height: None,
    })?;
    Some((width, intrinsic.height.ceil().max(1.0) as u32))
}

/// What one size-to-content frame does; see [`autosize_step`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AutosizeStep {
    /// The size to ask the window system for, and to make the window's
    /// minimum: the user may not shrink it below its content, and a
    /// programmatic shrink (a details expander folding) lowers the minimum
    /// with it. `None` asks nothing.
    pub(crate) ask: Option<(u32, u32)>,
    /// The centre the dialog was opened on no longer applies
    /// ([`autosize_centre`](crate::window_manager::ManagedWindow::autosize_centre)):
    /// its size settled, or the window system answered the last request with
    /// another size (it refused it, or clamped it to the screen), so a later
    /// change keeps the corner.
    pub(crate) centre_ends: bool,
}

/// One size-to-content frame of a window: `now` its size, `target` the size
/// its content wants ([`size_to_content_target`]), `asked` the size last
/// asked for and `asked_from` the size the window had then. A request still
/// in flight (the window at `asked_from`) is not asked again; one the window
/// system answered with a third size is not waited on.
pub(crate) fn autosize_step(
    now: (u32, u32),
    target: (u32, u32),
    asked: Option<(u32, u32)>,
    asked_from: Option<(u32, u32)>,
) -> AutosizeStep {
    if target == now {
        return AutosizeStep {
            ask: None,
            centre_ends: true,
        };
    }
    let answered_otherwise =
        asked.is_some_and(|asked| asked != now) && asked_from.is_some_and(|from| from != now);
    AutosizeStep {
        ask: (asked != Some(target)).then_some(target),
        centre_ends: answered_otherwise,
    }
}

#[cfg(test)]
mod autosize_step_tests {
    use super::{AutosizeStep, autosize_step};

    #[test]
    fn a_dialog_grows_to_its_content_once() {
        assert_eq!(
            autosize_step((300, 200), (340, 220), None, None),
            AutosizeStep {
                ask: Some((340, 220)),
                centre_ends: false
            },
            "asked, and centred on the size it grows to"
        );
        assert_eq!(
            autosize_step((300, 200), (340, 220), Some((340, 220)), Some((300, 200))),
            AutosizeStep {
                ask: None,
                centre_ends: false
            },
            "in flight: not asked again"
        );
        assert_eq!(
            autosize_step((340, 220), (340, 220), Some((340, 220)), Some((300, 200))),
            AutosizeStep {
                ask: None,
                centre_ends: true
            },
            "settled"
        );
    }

    /// The window system answered with another size (a screen too small for
    /// the content): the centre ends, so the next change keeps the corner,
    /// and the size it settled on is not asked for again.
    #[test]
    fn a_size_the_window_system_refuses_ends_the_centre() {
        assert_eq!(
            autosize_step((320, 210), (340, 220), Some((340, 220)), Some((300, 200))),
            AutosizeStep {
                ask: None,
                centre_ends: true
            }
        );
        assert_eq!(
            autosize_step((320, 210), (330, 230), Some((340, 220)), Some((300, 200))),
            AutosizeStep {
                ask: Some((330, 230)),
                centre_ends: true
            },
            "a new size its content wants then is asked without the centre"
        );
    }
}

fn modal_close_behavior_to_overlay_dismiss(behavior: ModalCloseBehavior) -> DismissBehavior {
    match behavior {
        ModalCloseBehavior::ClickOutside => DismissBehavior::ClickOutside,
        ModalCloseBehavior::EscapeKey => DismissBehavior::EscapeKey,
        ModalCloseBehavior::EscapeOrClickOutside => DismissBehavior::EscapeOrClickOutside,
        ModalCloseBehavior::Manual => DismissBehavior::Manual,
    }
}

/// Whether a modal presented as a **native OS window** dismisses on Escape.
///
/// The native presentation can honour only the Escape half of
/// [`ModalCloseBehavior`]: while the modal is up the parent window is
/// input-blocked by the OS, so there is no "outside" left to click. So
/// `ClickOutside` resolves to `false` here — the same answer as `Manual` —
/// rather than to a silently different second route.
fn native_modal_escape_dismisses(behavior: ModalCloseBehavior) -> bool {
    match behavior {
        ModalCloseBehavior::EscapeKey | ModalCloseBehavior::EscapeOrClickOutside => true,
        ModalCloseBehavior::ClickOutside | ModalCloseBehavior::Manual => false,
    }
}

/// Splice the Escape route around a native modal's root and return the new root.
///
/// Bubble, not preview: a focused descendant — and any overlay it owns, which
/// the tree dismisses before dispatch even begins — gets first refusal on
/// Escape, exactly as it does inside an in-tree modal. Only an unclaimed
/// Escape reaches this wrapper, which asks the window manager to close the
/// modal window.
fn wrap_native_modal_in_escape_route(tree: &mut WidgetTree, content_id: WidgetId) -> WidgetId {
    use teksilo_core::widget_builder::WidgetBuilder as _;
    tree.add(
        teksilo_widgets::ZStack::new()
            .child(content_id)
            .on_key(|event, ctx| match event {
                teksilo_core::event::WidgetEvent::KeyDown {
                    key: teksilo_core::event::Key::Escape,
                    ..
                } => {
                    ctx.dismiss_modal();
                    teksilo_core::event::EventResponse::Handled
                }
                _ => teksilo_core::event::EventResponse::Ignored,
            }),
    )
}

fn present_in_tree_modal_request(
    tree: &mut WidgetTree,
    source_widget: WidgetId,
    request: ModalRequest,
) {
    let dismiss = modal_close_behavior_to_overlay_dismiss(request.close_behavior);
    let requested_focus = request.focus_target;
    let user_on_dismiss = request.on_dismiss;
    let close_behavior = request.close_behavior;
    let placement = match request.placement {
        teksilo_core::modal::ModalPlacement::Centered => OverlayPlacement::Centered,
        teksilo_core::modal::ModalPlacement::Top { offset } => {
            OverlayPlacement::TopCenter { offset }
        }
    };
    // Capture the focus owner BEFORE the modal moves focus into itself
    // (below). Recorded as the modal overlay's `focus_restore` so that
    // dismissing the dialog returns keyboard focus to the trigger — e.g.
    // tabbing to a "Rename…" button, opening the InputDialog, then
    // accepting/cancelling lands back on that button. Without this, the
    // modal shows via `show_overlay` (which, unlike
    // `show_overlay_from_source`, records no restore target) and focus is
    // dropped on dismiss.
    let focus_before_modal = tree.focused();
    // Capture the `:focus-visible` input modality at the same instant. A
    // modal is a transient interruption: when it closes and focus snaps
    // back to the trigger, the trigger's focus ring should look exactly as
    // it did before the modal opened — NOT inherit keyboard modality from
    // input directed *at the dialog* (typing a name, pressing Enter to
    // accept). Without restoring this, mouse-clicking the trigger then
    // pressing Enter inside the dialog leaves the global modality "keyboard"
    // and the trigger sprouts a focus ring it never had. We restore it on
    // dismiss alongside focus. (`focus_ops` itself never touches this
    // signal, so the value we restore is the value that sticks.)
    let focus_visible_before = tree.focus_visible_signal().get();
    let focus_visible_signal = tree.focus_visible_signal();
    let content_id = match request.content {
        ModalContent::ExistingWidget(id) => id,
        ModalContent::Deferred(builder) => {
            let id = builder(tree);
            tree.set_dormant(id);
            id
        }
    };

    // Mount the dialog scrim FIRST so it z-orders below the modal
    // panel in the overlay stack. The scrim chrome (a full-viewport
    // dim) comes from the active `DialogStyle::make_scrim`; clicks on
    // it dismiss the modal when its `ModalCloseBehavior` permits
    // click-outside dismissal. The framework patches the scrim's
    // `parent_overlay` after the modal is pushed so that dismissing
    // the modal cascades through and also dismisses the scrim.
    let click_to_dismiss = matches!(
        close_behavior,
        ModalCloseBehavior::ClickOutside | ModalCloseBehavior::EscapeOrClickOutside,
    );
    let dismiss_target: std::rc::Rc<std::cell::Cell<Option<teksilo_core::overlay::OverlayId>>> =
        std::rc::Rc::new(std::cell::Cell::new(None));
    let scrim_id = tree.add(
        teksilo_widgets::ModalScrim::new()
            .dismiss_target(dismiss_target.clone())
            .click_to_dismiss(click_to_dismiss),
    );
    let scrim_overlay = tree.show_overlay(OverlayRequest::new(
        scrim_id,
        source_widget,
        OverlayPlacement::FullViewport,
        DismissBehavior::Manual,
    ));

    tree.activate(content_id);
    // Wrap the caller's `on_dismiss` so the framework also restores the
    // pre-modal `:focus-visible` modality when the dialog closes (by any
    // path: OK, Cancel, Escape, click-outside). Only when a focus owner
    // was captured — if nothing was focused before, there's no prior state
    // to return to. The overlay fires `on_dismiss` during dismissal, just
    // before focus is restored to the trigger, so the value we set here is
    // the one the trigger paints with.
    let restore_modality = focus_before_modal.is_some();
    let on_dismiss: Option<teksilo_core::overlay::OverlayDismissCallback> =
        if restore_modality || user_on_dismiss.is_some() {
            Some(std::rc::Rc::new(
                move |reason, ctx: &mut teksilo_core::widget::EventContext| {
                    if restore_modality {
                        focus_visible_signal.set(focus_visible_before);
                    }
                    if let Some(cb) = &user_on_dismiss {
                        cb(reason, ctx);
                    }
                },
            ))
        } else {
            None
        };
    // Present the modal as a WINDOW-LEVEL overlay via `show_overlay` rather than
    // `show_overlay_from_source`: the latter re-parents the overlay to the source
    // widget's overlay ancestor, so a modal opened from a menu item would be
    // trapped in (and positioned relative to) the transient menu overlay instead
    // of centering on the window. Both modal placements ignore the anchor;
    // keeping `parent_overlay: None` places the modal against the viewport.
    let mut request = OverlayRequest::new(content_id, source_widget, placement, dismiss);
    request.on_dismiss = on_dismiss;
    let modal_overlay = tree.show_overlay(request);
    // The modal is now the topmost overlay; record where focus should
    // return when it dismisses. Mirrors `show_overlay_from_source`'s
    // capture-then-set-top pattern. The `is_active` guard on the restore
    // side makes a stale id (e.g. a menu trigger that went dormant) a
    // graceful no-op.
    if let Some(restore) = focus_before_modal {
        tree.overlay_manager_mut().set_top_focus_restore(restore);
    }
    // Cascade-dismiss the scrim when the modal is dismissed (by any
    // path: Escape, click-outside, manual). The scrim is below the
    // modal in the stack but counts as its "child" in the parent-
    // overlay graph, so `dismiss_immediate` walks the descendants and
    // dismisses it too.
    tree.overlay_manager_mut()
        .set_parent_overlay(scrim_overlay, Some(modal_overlay));
    // Fill in the dismiss target NOW that the modal id is known. The
    // scrim's on-tap reads through this `Cell` at click time.
    dismiss_target.set(Some(modal_overlay));

    let focus_target = requested_focus
        .filter(|id| tree.is_active(*id) && tree.is_descendant_of(*id, content_id))
        .or_else(|| tree.widget_initial_focus_hint(content_id))
        .or_else(|| tree.first_focusable_descendant(content_id));
    if let Some(id) = focus_target {
        tree.focus(id);
    }
}

/// How often the on-screen keyboard's rectangle is re-read. See
/// [`TeksiloAppHandler::refresh_occluded_band`].
const OSK_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// How long the loop sleeps at most while textures a renderer dropped wait
/// for the GPU to free them. See [`TeksiloAppHandler::update_control_flow`].
pub(crate) const GPU_RECLAIM_POLL: Duration = Duration::from_millis(8);

/// Convert a screen-space physical rectangle into the part of `window`'s client
/// area it covers, in window-logical pixels.
///
/// `None` when the rectangle misses the window entirely, which is the common
/// case for every window a keyboard is not in front of.
fn occluded_band_in_window(
    window: &winit::window::Window,
    screen: (i32, i32, i32, i32),
) -> Option<teksilo_canvas::Rect> {
    let origin = window.inner_position().ok()?;
    let size = window.inner_size();
    occluded_band_in_client(
        (origin.x, origin.y),
        (size.width, size.height),
        window.scale_factor() as f32,
        screen,
    )
}

/// The arithmetic behind [`occluded_band_in_window`], with the window replaced
/// by the three numbers it contributes — so the clipping can be checked on a
/// host with no keyboard, no display and no window.
fn occluded_band_in_client(
    client_origin: (i32, i32),
    client_size: (u32, u32),
    scale_factor: f32,
    (left, top, right, bottom): (i32, i32, i32, i32),
) -> Option<teksilo_canvas::Rect> {
    if scale_factor <= 0.0 {
        return None;
    }
    let x0 = (left - client_origin.0).max(0) as f32;
    let y0 = (top - client_origin.1).max(0) as f32;
    let x1 = (right - client_origin.0).min(client_size.0 as i32) as f32;
    let y1 = (bottom - client_origin.1).min(client_size.1 as i32) as f32;
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(teksilo_canvas::Rect::new(
        x0 / scale_factor,
        y0 / scale_factor,
        (x1 - x0) / scale_factor,
        (y1 - y0) / scale_factor,
    ))
}

fn apply_cursor_to_window(
    platform_window: &teksilo_platform::PlatformWindow,
    cursor: teksilo_core::CursorIcon,
) {
    let winit_cursor = match cursor {
        teksilo_core::CursorIcon::Default => winit::window::CursorIcon::Default,
        teksilo_core::CursorIcon::Pointer => winit::window::CursorIcon::Pointer,
        teksilo_core::CursorIcon::Text => winit::window::CursorIcon::Text,
        teksilo_core::CursorIcon::Crosshair => winit::window::CursorIcon::Crosshair,
        teksilo_core::CursorIcon::Move => winit::window::CursorIcon::Move,
        teksilo_core::CursorIcon::NotAllowed => winit::window::CursorIcon::NotAllowed,
        teksilo_core::CursorIcon::Grab => winit::window::CursorIcon::Grab,
        teksilo_core::CursorIcon::Grabbing => winit::window::CursorIcon::Grabbing,
        teksilo_core::CursorIcon::ColResize => winit::window::CursorIcon::ColResize,
        teksilo_core::CursorIcon::RowResize => winit::window::CursorIcon::RowResize,
        teksilo_core::CursorIcon::NeswResize => winit::window::CursorIcon::NeswResize,
        teksilo_core::CursorIcon::NwseResize => winit::window::CursorIcon::NwseResize,
    };
    platform_window.window().set_cursor(winit_cursor);
}

/// The real window behind [`crate::input_loop::InputChrome`].
///
/// Holds the platform window and, when one is running, the idle trace — the
/// two things the input half of a turn reaches for that a headless test has no
/// equivalent of. Everything with a decision in it lives in `input_loop`; this
/// is the adapter, and it is deliberately without one.
pub(crate) struct WindowChrome<'a> {
    window: &'a teksilo_platform::PlatformWindow,
    redraw: &'a crate::redraw_gate::RedrawGate,
    trace: Option<&'a mut IdleTrace>,
}

impl<'a> WindowChrome<'a> {
    /// Wrap `window` and its redraw gate, optionally noting redraw reasons
    /// into `trace`.
    pub(crate) fn new(
        window: &'a teksilo_platform::PlatformWindow,
        redraw: &'a crate::redraw_gate::RedrawGate,
        trace: Option<&'a mut IdleTrace>,
    ) -> Self {
        Self {
            window,
            redraw,
            trace,
        }
    }
}

impl crate::input_loop::InputChrome for WindowChrome<'_> {
    fn set_cursor(&mut self, icon: teksilo_core::CursorIcon) {
        apply_cursor_to_window(self.window, icon);
    }

    fn request_redraw(&mut self, reason: &'static str) {
        if let Some(trace) = &mut self.trace {
            trace.note_redraw_request(reason);
        }
        let window = self.window;
        self.redraw
            .request_with(Instant::now(), || window.request_redraw());
    }

    fn safe_area(&self) -> teksilo_platform::safe_area::SafeAreaSides {
        teksilo_platform::window_safe_area(self.window.window())
    }

    fn occluded_band(&self, screen: (i32, i32, i32, i32)) -> Option<teksilo_canvas::Rect> {
        occluded_band_in_window(self.window.window(), screen)
    }

    fn set_soft_keyboard_visible(&mut self, visible: bool) {
        teksilo_platform::soft_keyboard::set_visible(self.window.window(), visible);
    }

    fn set_ime(&mut self, purpose: Option<teksilo_core::ImePurpose>, allowed: Option<bool>) {
        // The adapter, and nothing else: which of the two to push, and whether
        // to push at all, is `input_loop::settle_ime`'s decision.
        if let Some(purpose) = purpose {
            self.window
                .window()
                .set_ime_purpose(TeksiloAppHandler::map_ime_purpose(purpose));
        }
        if let Some(allowed) = allowed {
            self.window.window().set_ime_allowed(allowed);
        }
    }
}

#[derive(Debug)]
pub(crate) struct IdleTrace {
    started: Instant,
    last_report: Instant,
    /// `RedrawRequested` events a hidden window answered without drawing.
    hidden_redraws: u64,
    /// Non-visual frames run from `about_to_wait`: for windows that draw
    /// nothing (hidden, or their redraw withheld), and state ticks.
    ticks: u64,
    resume_time_reached: u64,
    redraw_requested: u64,
    rendered_frames: u64,
    request_redraw_all: u64,
    cursor_redraw_requests: u64,
    mouse_input_redraw_requests: u64,
    mouse_wheel_redraw_requests: u64,
    keyboard_redraw_requests: u64,
    resize_redraw_requests: u64,
    /// Post-render redraw requests caused by `tree.frame_requested()`
    /// (a widget asked for another frame from a `frame_tick` effect or
    /// similar). Surfaces the only redraw source that was previously
    /// invisible to the trace.
    frame_request_redraws: u64,
    /// Windows poked by `WindowManager::request_redraw_needing_render`
    /// (a sibling window dirtied by another window's `Signal` mutation),
    /// distinct from `request_redraw_all` — surfaces how often the
    /// targeted cross-window path actually fires versus the blanket one.
    cross_window_redraws: u64,
    idle_callbacks_run: u64,
    /// App events that reached the public dispatch (`on_app_event` and the
    /// framework's arms); a framework-private wake is not one.
    app_events: u64,
    /// Wakes a window's wake target posted to the event loop, by kind.
    posted_draw_wakes: u64,
    posted_layout_wakes: u64,
    /// Window wakes routed, and draw wakes dropped because the window drew
    /// nothing: deltas of every window's `LiveWakeStats`.
    waker_wakes: u64,
    waker_wakes_dropped: u64,
    /// Each open window's `LiveWakeStats` at the last sample, to take
    /// deltas from.
    waker_totals: HashMap<WindowId, (u64, u64)>,
    control_flow_wait: u64,
    control_flow_wait_until: u64,
    /// Control-flow decisions that kept a deadline for the GPU reclaim poll.
    gpu_reclaim_waits: u64,
    timer_windows: usize,
    animation_timers: usize,
    tooltip_timers: usize,
    #[cfg(feature = "live-image-timings")]
    live: LiveTrace,
}

/// What the idle trace prints of live pictures, with the
/// `live-image-timings` feature: after each report, a line per window that
/// rendered since the one before and whose last frame drew a live picture.
#[cfg(feature = "live-image-timings")]
#[derive(Debug, Default)]
struct LiveTrace {
    /// Windows that rendered a frame since the last report.
    rendered: std::collections::HashSet<WindowId>,
    /// The windows the last report owes a line, until it is printed.
    due: Vec<WindowId>,
    /// Each window's uploads, copies and contended frames when it was last
    /// read, to print deltas from.
    totals: HashMap<WindowId, (u64, u64, u64)>,
}

/// The idle trace's live line for one window, at `t` seconds: its textures,
/// the uploads, copies of another window's texture and contended frames
/// since `last` (the window's when it was last read), and its timings in
/// µs.
#[cfg(feature = "live-image-timings")]
fn live_trace_line(
    t: f64,
    window: impl std::fmt::Debug,
    stats: &teksilo_render::LiveTextureStats,
    timings: &teksilo_render::LiveImageTimings,
    last: (u64, u64, u64),
) -> String {
    let percentiles = |p: teksilo_render::Percentiles| {
        format!(
            "{{p50:{},p90:{},p99:{},max:{},n:{}}}",
            p.p50, p.p90, p.p99, p.max, p.samples
        )
    };
    let uploads = stats.uploads_full + stats.uploads_partial;
    format!(
        "teksilo_idle_trace_live t={t:.3} window={window:?} textures={} bytes={} uploads={} copies={} contended={} prepare_us={} lock_hold_us={} commit_to_upload_us={}",
        stats.textures,
        stats.bytes,
        uploads.saturating_sub(last.0),
        stats.sibling_copies.saturating_sub(last.1),
        stats.contended.saturating_sub(last.2),
        percentiles(timings.prepare),
        percentiles(timings.lock_hold),
        percentiles(timings.commit_to_upload),
    )
}

impl IdleTrace {
    fn from_env() -> Option<Self> {
        match std::env::var("TEKSILO_IDLE_TRACE") {
            Ok(value) if value != "0" && !value.is_empty() => Some(Self::new()),
            _ => None,
        }
    }

    fn new() -> Self {
        Self {
            started: Instant::now(),
            last_report: Instant::now(),
            hidden_redraws: 0,
            ticks: 0,
            resume_time_reached: 0,
            redraw_requested: 0,
            rendered_frames: 0,
            request_redraw_all: 0,
            cursor_redraw_requests: 0,
            mouse_input_redraw_requests: 0,
            mouse_wheel_redraw_requests: 0,
            keyboard_redraw_requests: 0,
            resize_redraw_requests: 0,
            frame_request_redraws: 0,
            cross_window_redraws: 0,
            idle_callbacks_run: 0,
            app_events: 0,
            posted_draw_wakes: 0,
            posted_layout_wakes: 0,
            waker_wakes: 0,
            waker_wakes_dropped: 0,
            waker_totals: HashMap::new(),
            control_flow_wait: 0,
            control_flow_wait_until: 0,
            gpu_reclaim_waits: 0,
            timer_windows: 0,
            animation_timers: 0,
            tooltip_timers: 0,
            #[cfg(feature = "live-image-timings")]
            live: LiveTrace::default(),
        }
    }

    fn note_control_flow(
        &mut self,
        has_deadline: bool,
        timer_windows: usize,
        animation_timers: usize,
        tooltip_timers: usize,
        gpu_reclaim: bool,
    ) {
        if has_deadline {
            self.control_flow_wait_until += 1;
        } else {
            self.control_flow_wait += 1;
        }
        if gpu_reclaim {
            self.gpu_reclaim_waits += 1;
        }
        self.timer_windows = timer_windows;
        self.animation_timers = animation_timers;
        self.tooltip_timers = tooltip_timers;
        self.maybe_report();
    }

    fn note_request_redraw_all(&mut self) {
        self.request_redraw_all += 1;
        self.maybe_report();
    }

    pub(crate) fn note_redraw_request(&mut self, reason: &'static str) {
        match reason {
            "cursor" => self.cursor_redraw_requests += 1,
            "mouse_input" => self.mouse_input_redraw_requests += 1,
            "mouse_wheel" => self.mouse_wheel_redraw_requests += 1,
            "keyboard" => self.keyboard_redraw_requests += 1,
            "resize" => self.resize_redraw_requests += 1,
            _ => {}
        }
        self.maybe_report();
    }

    fn note_cross_window_redraw(&mut self, windows: usize) {
        self.cross_window_redraws += windows as u64;
        self.maybe_report();
    }

    fn note_resume_time_reached(&mut self) {
        self.resume_time_reached += 1;
        self.maybe_report();
    }

    fn note_redraw_requested(&mut self) {
        self.redraw_requested += 1;
        self.maybe_report();
    }

    fn note_rendered_frame(&mut self, window: WindowId) {
        self.rendered_frames += 1;
        #[cfg(feature = "live-image-timings")]
        self.live.rendered.insert(window);
        #[cfg(not(feature = "live-image-timings"))]
        let _ = window;
        self.maybe_report();
    }

    fn note_hidden_redraw(&mut self) {
        self.hidden_redraws += 1;
        self.maybe_report();
    }

    fn note_tick(&mut self) {
        self.ticks += 1;
        self.maybe_report();
    }

    fn note_idle_callbacks_run(&mut self) {
        self.idle_callbacks_run += 1;
        self.maybe_report();
    }

    fn note_app_event(&mut self) {
        self.app_events += 1;
        self.maybe_report();
    }

    fn note_posted_wake(&mut self, kind: teksilo_canvas::wake::WakeKind) {
        match kind {
            teksilo_canvas::wake::WakeKind::Draw => self.posted_draw_wakes += 1,
            _ => self.posted_layout_wakes += 1,
        }
        self.maybe_report();
    }

    /// Fold in the wake counters of every open window, each against what
    /// was last seen of that window: they are monotonic per window, and a
    /// window that closed is forgotten without taking another's counts with
    /// it.
    fn note_wake_totals(&mut self, windows: impl Iterator<Item = (WindowId, u64, u64)>) {
        let mut seen = HashMap::new();
        for (id, wakes, dropped) in windows {
            let (last_wakes, last_dropped) = self.waker_totals.get(&id).copied().unwrap_or((0, 0));
            self.waker_wakes += wakes.saturating_sub(last_wakes);
            self.waker_wakes_dropped += dropped.saturating_sub(last_dropped);
            seen.insert(id, (wakes, dropped));
        }
        self.waker_totals = seen;
    }

    fn maybe_report(&mut self) {
        if self.last_report.elapsed() < Duration::from_secs(1) {
            return;
        }

        eprintln!(
            "teksilo_idle_trace t={:.3} redraw_requested={} rendered_frames={} hidden_redraws={} ticks={} resume_time_reached={} request_redraw_all={} cross_window_redraws={} input_redraws={{cursor:{},mouse_input:{},mouse_wheel:{},keyboard:{},resize:{},frame_request:{}}} idle_callbacks={} app_events={} posted_wakes={{draw:{},layout:{}}} waker_wakes={} waker_wakes_dropped={} control_flow={{wait:{},wait_until:{},gpu_reclaim:{}}} timers={{windows:{},animations:{},tooltips:{}}}",
            self.started.elapsed().as_secs_f64(),
            self.redraw_requested,
            self.rendered_frames,
            self.hidden_redraws,
            self.ticks,
            self.resume_time_reached,
            self.request_redraw_all,
            self.cross_window_redraws,
            self.cursor_redraw_requests,
            self.mouse_input_redraw_requests,
            self.mouse_wheel_redraw_requests,
            self.keyboard_redraw_requests,
            self.resize_redraw_requests,
            self.frame_request_redraws,
            self.idle_callbacks_run,
            self.app_events,
            self.posted_draw_wakes,
            self.posted_layout_wakes,
            self.waker_wakes,
            self.waker_wakes_dropped,
            self.control_flow_wait,
            self.control_flow_wait_until,
            self.gpu_reclaim_waits,
            self.timer_windows,
            self.animation_timers,
            self.tooltip_timers,
        );

        self.last_report = Instant::now();
        self.resume_time_reached = 0;
        self.redraw_requested = 0;
        self.rendered_frames = 0;
        self.hidden_redraws = 0;
        self.ticks = 0;
        self.request_redraw_all = 0;
        self.cross_window_redraws = 0;
        self.cursor_redraw_requests = 0;
        self.mouse_input_redraw_requests = 0;
        self.mouse_wheel_redraw_requests = 0;
        self.keyboard_redraw_requests = 0;
        self.resize_redraw_requests = 0;
        self.frame_request_redraws = 0;
        self.idle_callbacks_run = 0;
        self.app_events = 0;
        self.posted_draw_wakes = 0;
        self.posted_layout_wakes = 0;
        self.waker_wakes = 0;
        self.waker_wakes_dropped = 0;
        self.control_flow_wait = 0;
        self.control_flow_wait_until = 0;
        self.gpu_reclaim_waits = 0;
        #[cfg(feature = "live-image-timings")]
        {
            self.live.due = self.live.rendered.drain().collect();
        }
    }

    /// Print the live line the last report owes each window it saw render:
    /// the window's live-pass timings (each over its latest 1,024 samples,
    /// in µs), its textures, and the uploads and contended frames since it
    /// was last read. Only those windows' timings are read, and only once
    /// per report. `windows` is every open window.
    #[cfg(feature = "live-image-timings")]
    fn report_live<'a>(
        &mut self,
        windows: impl Iterator<Item = (WindowId, &'a teksilo_platform::PlatformWindow)>,
    ) {
        if self.live.due.is_empty() {
            return;
        }
        let due = std::mem::take(&mut self.live.due);
        let t = self.started.elapsed().as_secs_f64();
        let mut totals = HashMap::new();
        for (id, window) in windows {
            let last = self.live.totals.get(&id).copied();
            if !due.contains(&id) {
                if let Some(last) = last {
                    totals.insert(id, last);
                }
                continue;
            }
            let stats = window.live_texture_stats();
            totals.insert(
                id,
                (
                    stats.uploads_full + stats.uploads_partial,
                    stats.sibling_copies,
                    stats.contended,
                ),
            );
            if stats.textures == 0 {
                continue;
            }
            let timings = window.live_texture_timings();
            eprintln!(
                "{}",
                live_trace_line(t, id, &stats, &timings, last.unwrap_or_default())
            );
        }
        self.live.totals = totals;
    }
}

/// Ten a second is fast enough that a magnifier tracking a scroll never looks
/// stuck, and slow enough that a flung list does not bury AT-SPI under a
/// bounds-changed signal per node per frame.
const MOVE_DELIVERY_INTERVAL: Duration = Duration::from_millis(100);

/// What a frame does with the accessibility tree while an assistive
/// technology is attached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum A11yDelivery {
    /// Hand the adapter the tree.
    Deliver,
    /// Something is owed, but the throttle holds it back: ask for a frame
    /// then, since nothing else may wake the loop.
    HoldUntil(Instant),
    /// The adapter already holds the tree as it is: delivering would hand it
    /// a copy to compare node by node, and holding back would cost a frame.
    Nothing,
}

/// Decide [`A11yDelivery`] for a frame. `owed`: the adapter lacks something
/// (`WidgetTree::accessibility_delivery_owed`). `needs_full`: the adapter
/// asked for the whole tree. `walk_changed`: the tree was walked since the
/// last delivery. A walk, or the first delivery, goes at once; moves alone
/// go at most every [`MOVE_DELIVERY_INTERVAL`].
fn a11y_delivery(
    owed: bool,
    needs_full: bool,
    walk_changed: bool,
    delivered_at: Option<Instant>,
    now: Instant,
) -> A11yDelivery {
    if needs_full {
        return A11yDelivery::Deliver;
    }
    if !owed {
        return A11yDelivery::Nothing;
    }
    match delivered_at {
        Some(at) if !walk_changed && now.duration_since(at) < MOVE_DELIVERY_INTERVAL => {
            A11yDelivery::HoldUntil(at + MOVE_DELIVERY_INTERVAL)
        }
        _ => A11yDelivery::Deliver,
    }
}

/// An app-supplied router for [`AppEvent::External`] payloads that need to
/// perform **window operations** — open a window, focus one, look one up by its
/// string id.
///
/// Registered with [`TeksiloAppBuilder::on_external_with_ctx`]. Returns `true`
/// to say "this payload was mine"; `false` leaves it unclaimed.
///
/// The plain [`on_app_event`](TeksiloAppBuilder::on_app_event) hook receives only
/// `&AppEvent` — no tree, no [`WindowOps`](teksilo_core::WindowOps) — so a handler
/// there cannot call `open_window` at all: `EventContext::open_window` panics on a
/// standalone context. This one runs against a real window's tree with a real ops
/// sink, which is what makes the multi-window recipes in `docs/multi-window.md`
/// reachable from a background thread (a single-instance app's IPC listener being
/// the motivating case: a second launch forwards its command line and the running
/// process opens the document window).
pub type ExternalCtxHandler =
    Box<dyn FnMut(&(dyn std::any::Any + Send), &mut teksilo_core::widget::EventContext) -> bool>;

struct TeksiloAppHandler {
    wm: WindowManager,
    app_event_handler: Option<Box<dyn FnMut(&AppEvent)>>,
    /// App-supplied `AppEvent::External` router with window ops — see
    /// [`ExternalCtxHandler`]. Consulted only for payloads no framework router
    /// and no built-in downcast arm claimed.
    external_ctx_handler: Option<ExternalCtxHandler>,
    initial_window: Option<WindowConfig>,
    initial_created: bool,
    /// The first `resumed` has run, and with it the startup check of
    /// [`QuitPolicy::refuses_startup_without_window`].
    started: bool,
    /// When the loop ends on its own; see [`QuitPolicy`].
    quit_policy: QuitPolicy,
    /// The intents dropped by the routing between trees that a debug build
    /// has already named.
    dropped_intents: crate::intent_routing::DroppedIntents,
    /// A debug build said once that the application tree holds something
    /// only a drawn tree shows: an animated widget or an overlay.
    #[cfg(debug_assertions)]
    warned_visual_app_tree: bool,
    idle_budget: Duration,
    idle_trace: Option<IdleTrace>,
    /// The GPU reclaim poll, [`teksilo_render::poll_gpu_reclaim`]: a field
    /// so the loop test can make it report pending.
    gpu_reclaim: fn() -> bool,
    typesetter: SharedTypesetter,
    /// Kept alive for the lifetime of the event loop so that the
    /// `notify::RecommendedWatcher` background thread keeps running.
    /// Created in `TeksiloAppBuilder::run` when the `I18nConfig` registers
    /// any `runtime_override`s; otherwise `None`.
    _i18n_watcher: Option<teksilo_i18n::FtlFileWatcher>,
    /// Kept alive for the lifetime of the event loop so that the
    /// settings directory watcher's background thread keeps running.
    /// Created in `TeksiloAppBuilder::run` when a settings bundle was
    /// opened and live-reload was not disabled; otherwise `None`.
    _settings_watcher: Option<teksilo_settings::SettingsWatcher>,
    /// Optional per-loop-turn closure (e.g. an async executor poll) installed
    /// via [`TeksiloAppBuilder::on_loop_tick`]. Runs at the top of
    /// `about_to_wait`; returning `true` means tasks advanced and a repaint is
    /// needed. Async-agnostic — the loop only ever sees `FnMut`.
    loop_tick: Option<Box<dyn FnMut() -> bool>>,
    /// Shared flag a `loop_tick` owner sets while it wants continuous polling.
    /// Read in `update_control_flow` to force `ControlFlow::Poll`; when clear,
    /// the loop sleeps until the next event (off-thread wakes via the proxy).
    loop_tick_poll: Option<std::rc::Rc<std::cell::Cell<bool>>>,
    /// Whether the last loop turn was started by the OS rather than by our own
    /// `WaitUntil`. Read by the pen pump: an external wake with a digitizer
    /// attached is the moment a packet may be one shim-poll away.
    woken_externally: bool,
    /// One-shot catch-up deadline for the pen shim. See
    /// [`TeksiloAppHandler::pen_deadline`].
    pen_recheck_at: Option<Instant>,
    /// When the on-screen keyboard's rectangle was last read. See
    /// [`TeksiloAppHandler::refresh_occluded_band`].
    osk_polled_at: Option<Instant>,
    /// Tells an attached screen reader of each key before the application
    /// acts on it, where the reader cannot see the keyboard itself (Orca
    /// under Wayland), and holds back the keys the reader took. One per
    /// application: a key pressed in one window can come up in another.
    key_report: teksilo_platform::key_report::KeyReportGate,
    /// The settings the builder opened, flushed by
    /// [`flush_settings`](Self::flush_settings) on every way out: the
    /// operating system may end the process before `run` returns.
    settings: Option<teksilo_settings::OpenedSettings>,
    /// Whether `resumed` starts [`quit_signals`](Self::quit_signals); see
    /// [`TeksiloAppBuilder::termination_signals`].
    termination_signals: bool,
    /// `SIGTERM` and `SIGHUP` turned into a quit while the loop runs
    /// (Unix), each unless the process ignored it when the watch started.
    #[cfg(unix)]
    quit_signals: Option<teksilo_platform::os_quit::SignalWatch>,
    /// AppKit's `terminate:` asks the application (macOS).
    #[cfg(target_os = "macos")]
    terminate_hooked: bool,
    /// Hears the session end (Windows).
    #[cfg(target_os = "windows")]
    session_end_watch: Option<teksilo_platform::os_quit::SessionEndWatch>,
}

impl TeksiloAppHandler {
    fn new(
        theme: Theme,
        theme_mode: ThemeMode,
        app_event_handler: Option<Box<dyn FnMut(&AppEvent)>>,
        initial_window: Option<WindowConfig>,
        quit_policy: QuitPolicy,
        app_context_template: Option<std::rc::Rc<TreeAppContext>>,
        typesetter: SharedTypesetter,
        i18n_watcher: Option<teksilo_i18n::FtlFileWatcher>,
        settings_watcher: Option<teksilo_settings::SettingsWatcher>,
        event_proxy: AppEventProxy,
    ) -> Self {
        let mut wm = WindowManager::new(theme);
        wm.set_theme_mode(theme_mode);
        wm.set_event_proxy(event_proxy);
        if let Some(template) = app_context_template {
            // Seed the persisted user text-scale factor (if settings are
            // installed) so every initially-created window opens at the saved
            // scale. No per-app boilerplate: apps without settings stay at 1.0.
            if let Some(store) = template.app_state::<teksilo_settings::SettingsStore>() {
                let scale = store.signal_for(&teksilo_settings::TEXT_SCALE_KEY).get();
                wm.set_initial_text_scale(scale);
            }
            wm.set_app_context_template(template);
        }

        wm.set_typesetter(typesetter.clone());

        Self {
            wm,
            app_event_handler,
            external_ctx_handler: None,
            initial_window,
            initial_created: false,
            started: false,
            quit_policy,
            dropped_intents: Default::default(),
            #[cfg(debug_assertions)]
            warned_visual_app_tree: false,
            idle_budget: Duration::from_millis(4),
            idle_trace: IdleTrace::from_env(),
            gpu_reclaim: teksilo_render::poll_gpu_reclaim,
            typesetter,
            _i18n_watcher: i18n_watcher,
            _settings_watcher: settings_watcher,
            loop_tick: None,
            loop_tick_poll: None,
            woken_externally: true,
            pen_recheck_at: None,
            osk_polled_at: None,
            key_report: teksilo_platform::key_report::KeyReportGate::for_platform(),
            settings: None,
            termination_signals: false,
            #[cfg(unix)]
            quit_signals: None,
            #[cfg(target_os = "macos")]
            terminate_hooked: false,
            #[cfg(target_os = "windows")]
            session_end_watch: None,
        }
    }

    /// Listen for the operating system's own requests to quit, once:
    /// AppKit's `terminate:` on macOS, the session ending on Windows,
    /// termination signals on Unix.
    fn watch_os_quit(&mut self) {
        #[cfg(target_os = "windows")]
        if self.session_end_watch.is_none()
            && let Some(proxy) = self.wm.event_proxy().cloned()
        {
            let settings = self.settings.clone();
            self.session_end_watch = teksilo_platform::os_quit::SessionEndWatch::new(
                move |request| {
                    teksilo_core::AppEventPoster::post_external(&proxy, Box::new(request));
                },
                // The loop did not get to the request in time (its handler
                // is busy in a native modal of the application's own): the
                // settings at least are written before the process ends.
                move || flush_settings_on_exit(settings.as_ref()),
            );
        }
        #[cfg(target_os = "macos")]
        if !self.terminate_hooked
            && let Some(proxy) = self.wm.event_proxy().cloned()
        {
            self.terminate_hooked =
                teksilo_platform::os_quit::install_terminate_hook(move |request| {
                    teksilo_core::AppEventPoster::post_external(&proxy, Box::new(request));
                });
        }
        #[cfg(unix)]
        if self.termination_signals && self.quit_signals.is_none() {
            let proxy = self.wm.event_proxy().cloned();
            if let Some(proxy) = proxy {
                match teksilo_platform::os_quit::SignalWatch::new(move |signal| {
                    teksilo_core::AppEventPoster::post_external(&proxy, Box::new(signal));
                }) {
                    Ok(watch) => self.quit_signals = Some(watch),
                    Err(e) => eprintln!(
                        "teksilo-app: termination signals keep their default meaning: {e}"
                    ),
                }
            }
        }
    }

    /// Write every pending settings change to disk now: the bundle's store
    /// and window state, if the application has a bundle, and the settings
    /// files the application opened itself, whether it has one or not.
    /// Called where the process may end before `run` returns (a
    /// quit with no time to ask, winit's `exiting`, which AppKit reaches
    /// just before it ends the process) and once more after the loop; a
    /// flush with nothing pending writes nothing.
    pub(crate) fn flush_settings(&self) {
        flush_settings_on_exit(self.settings.as_ref());
    }

    /// Quit now, for a source with no time to ask (a session ending now,
    /// Windows' `WM_ENDSESSION`, a termination signal): an overruling quit
    /// on behalf of `source`, carried out before this returns. The
    /// application's guard and every window's are asked and read `source`
    /// through `ctx.quit_source()`, so a document's guard can save without
    /// asking; whatever a guard answers, its window closes, and every
    /// dialog still up, or asked and not shown yet, answers through its own
    /// Cancel path as its window closes. A quit already under way is made
    /// overruling: an answer already given stands (a window whose guard
    /// already proceeded closes unasked, an application guard that already
    /// agreed is not asked again), and a pending one is asked again with
    /// `source`. When it returns every window is closed, the quit has
    /// completed, the settings are on disk, and the loop ends at the end of
    /// the turn, unless guards kept opening windows as theirs closed: the
    /// quit gives up after 4096 steps in one turn, says so on standard
    /// error, and returns with windows still open (a Windows session end is
    /// told it is finished all the same).
    pub(crate) fn quit_now(
        &mut self,
        source: crate::quit::QuitSource,
        event_loop: &ActiveEventLoop,
    ) {
        self.wm
            .request_quit_from(source, crate::quit::QuitMode::Overruling, None);
        self.process_pending(event_loop);
        self.flush_settings();
    }

    /// AppKit asked whether the application may end (a Dock Quit,
    /// `terminate:`, AppleScript's `quit`, a logout): a guarded quit on its
    /// behalf, `SessionEnd` for a logout, restart or shutdown and
    /// `UserQuit` otherwise, which joins a quit already under way. AppKit
    /// hears the outcome once the quit is decided; told the application
    /// ends, it ends the process, so the settings are written first.
    fn os_terminate_request(&mut self, request: teksilo_platform::os_quit::TerminateRequest) {
        let source = if request.session_end() {
            crate::quit::QuitSource::SessionEnd
        } else {
            crate::quit::QuitSource::UserQuit
        };
        let settings = self.settings.clone();
        self.wm.request_quit_from(
            source,
            crate::quit::QuitMode::Guarded,
            Some(Box::new(move |outcome| {
                let completed = outcome == crate::quit::QuitOutcome::Completed;
                if completed {
                    flush_settings_on_exit(settings.as_ref());
                }
                request.reply(completed);
            })),
        );
    }

    fn process_pending(&mut self, event_loop: &ActiveEventLoop) {
        // A close a tree asked for since the last drain (input dispatched
        // in `about_to_wait`, such as a pen tap or a timed gesture, is not
        // followed by `post_event`'s drain) is queued first, so the quit
        // sees a dialog's answer that closed its window, not the window
        // left without its dialog.
        self.wm.drain_close_window_requests();
        self.wm.process_pending(event_loop);
        self.wm.advance_quit(event_loop);
    }

    /// Present the modal requests the windows' trees queued, each over the
    /// window that asked, in its tree or as a native modal child as the
    /// request resolves.
    ///
    /// Each is presented as the application tree's is
    /// ([`app_dialog_presentation`](crate::window_manager::app_dialog_presentation)):
    /// raised, if native, when asked in answer to the user's input or while
    /// its window or a dialog of it has the focus
    /// ([`knows_focused_with_dialogs`](crate::window_manager::WindowManager::knows_focused_with_dialogs)).
    /// Asked otherwise, it opens without taking the focus and the window
    /// asks for attention, once for the dialog; a native one over its
    /// window minimised is owed: not shown, the window asking for
    /// attention, and presented, raised, on the first turn that window
    /// takes the focus (see
    /// [`owe_window_dialog`](crate::window_manager::WindowManager::owe_window_dialog)).
    /// The owed ones due come first, and a native one asked while its window
    /// owes some waits with them, so the order they were asked in holds,
    /// unless it is raised: one for the user (their input, a quit's
    /// question) is presented at once, its window restored first if
    /// minimised, as the application tree's is. A dialog in the window's
    /// own tree waits there, minimised with it.
    fn process_modal_requests(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let native_supported = native_modal_windows_supported();
        let due = self.wm.take_due_owed_dialogs();
        let requests = self.wm.drain_pending_modal_requests();
        let had_requests = !requests.is_empty() || !due.is_empty();

        // The user is back in the window: what it owes them comes forward.
        for (window, owed) in due {
            for request in owed {
                self.wm.set_raise_next_modal(true);
                self.open_modal_window(Some(window), request, event_loop);
                self.wm.set_raise_next_modal(false);
            }
        }

        let for_user = self.wm.acts_for_user() || self.wm.quit_awaits_answer();
        if !requests.is_empty() {
            self.wm.refresh_minimised_windows();
        }
        for (source_window, requests) in requests {
            for queued in requests {
                let resolved = resolve_modal_presentation(
                    queued.request.presentation,
                    &queued.request.content,
                    native_supported,
                );

                match resolved {
                    ResolvedModalPresentation::InTree => {
                        // Waiting in its tree over a window the user is not
                        // in (minimised with it, or behind), it asks for
                        // their attention as a native one does.
                        if crate::window_manager::app_dialog_presentation(
                            for_user,
                            self.wm.knows_focused_with_dialogs(source_window),
                            self.wm.knows_minimised(source_window),
                            false,
                        ) == crate::window_manager::AppDialogPresentation::Quiet
                        {
                            self.wm.ask_attention_for_dialog(source_window);
                        }
                        if let Some(managed) = self.wm.get_by_teksilo_mut(source_window) {
                            present_in_tree_modal_request(
                                &mut managed.tree,
                                queued.source_widget,
                                queued.request,
                            );
                        }
                    }
                    ResolvedModalPresentation::NativeWindow => {
                        use crate::window_manager::AppDialogPresentation;
                        let presentation = match crate::window_manager::app_dialog_presentation(
                            for_user,
                            self.wm.knows_focused_with_dialogs(source_window),
                            self.wm.knows_minimised(source_window),
                            true,
                        ) {
                            // One for the user (their input, a quit's
                            // question) goes now; any other waits behind
                            // those the window owes, so their order holds.
                            AppDialogPresentation::Raise => AppDialogPresentation::Raise,
                            _ if self.wm.owes_window_dialogs(source_window) => {
                                AppDialogPresentation::Owe
                            }
                            presentation => presentation,
                        };
                        match presentation {
                            AppDialogPresentation::Owe => {
                                self.wm.owe_window_dialog(source_window, queued.request);
                            }
                            AppDialogPresentation::Raise | AppDialogPresentation::Quiet => {
                                if presentation == AppDialogPresentation::Quiet {
                                    self.wm.ask_attention_for_dialog(source_window);
                                } else {
                                    // Its window comes back first, as the
                                    // application tree's does: a dialog is
                                    // shown only with the window it blocks.
                                    self.wm.restore_with_owners(source_window);
                                }
                                self.wm.set_raise_next_modal(
                                    presentation == AppDialogPresentation::Raise,
                                );
                                self.open_modal_window(
                                    Some(source_window),
                                    queued.request,
                                    event_loop,
                                );
                                self.wm.set_raise_next_modal(false);
                            }
                        }
                    }
                }
            }
        }

        had_requests
    }

    /// Open a native window presenting a [`ModalRequest`] with deferred
    /// content: a modal child of `parent`, or, with no parent (a request
    /// from the application tree with no window open), a plain window that
    /// the request's dismissal closes like a modal one. A request whose
    /// content is an existing widget cannot move to another tree and opens
    /// nothing.
    fn open_modal_window(
        &mut self,
        parent: Option<TeksiloWindowId>,
        request: ModalRequest,
        event_loop: &ActiveEventLoop,
    ) {
        let ModalRequest {
            content,
            title,
            size,
            focus_target,
            close_behavior,
            on_dismiss,
            ..
        } = request;

        let ModalContent::Deferred(builder) = content else {
            return;
        };

        let mut config = match parent {
            Some(parent) => WindowConfig::new().modal(crate::window_config::ModalConfig {
                parent,
                focus_target,
            }),
            // A dialog with no window to block is not modal, and would get
            // the buttons of an ordinary window.
            None => WindowConfig::new().minimizable(false).maximizable(false),
        };
        if let Some(title) = title {
            config = config.title(title);
        }
        if let Some((width, height)) = size {
            // Native modals size to their content on both axes (the
            // width through `autosize_width_floor`, set below): the
            // requested (width, height) is the floor, and the OS window
            // grows to fit content that is taller (a MessageBox "Show
            // details" expander) or wider (a dialog panel's padding
            // around content of the requested width). Without this the
            // fixed size clips the content: the footer buttons fall
            // below the client edge, or the panel is centred past the
            // window's sides with its buttons against the edges. NOTE:
            // deliberately NOT `resizable(false)`: winit encodes that as
            // min==max size hints on X11, which would clamp away the
            // programmatic growth this relies on.
            config = config
                .size(width, height)
                .min_size(width, height)
                .size_to_content(SizeToContent::Height);
        }
        let width_floor = size.map(|(width, _)| width);
        // Honour the request's `ModalCloseBehavior`. Only the
        // Escape half of it is expressible for a native
        // window: there is no "outside" to click, because the
        // parent window is input-blocked for as long as the
        // modal is up, which is what the OS does with a click
        // there, and is why `ClickOutside` resolves to
        // "nothing dismisses this but the app" here rather
        // than to a second route. Before this the whole field
        // fell into the struct's `..` and was never read, so a
        // `Dialog` taking BOTH defaults (`Auto` presentation,
        // `EscapeOrClickOutside`) got neither behaviour on
        // every platform that hosts a native modal.
        let escape_dismisses = native_modal_escape_dismisses(close_behavior);
        let id = self.wm.create_window(
            config.root(move |tree, _state| {
                let content_id = builder(tree);
                if escape_dismisses {
                    wrap_native_modal_in_escape_route(tree, content_id)
                } else {
                    content_id
                }
            }),
            event_loop,
        );
        if parent.is_none() {
            self.wm.mark_modal_host(id);
        }
        // Run as the window closes, however it closes (see
        // `WindowManager::close_window_answering`).
        if let Some(managed) = self.wm.get_by_teksilo_mut(id) {
            managed.modal_on_dismiss = on_dismiss;
            managed.autosize_width_floor = width_floor;
        }
    }

    /// Present the modal requests the application tree queued. It has no
    /// window to show them in, so each goes to the window the user is
    /// working with: the [most recent](crate::WindowManager::most_recent_window)
    /// one not about to close and not minimised, else the most recent one;
    /// over its frontmost native dialog if it has one, in its tree or as a
    /// native modal child as the request resolves. With no window open, or
    /// every one closing, it gets a plain window of its own, which its
    /// dismissal closes. A request whose content is an existing widget of
    /// the application tree cannot move to another tree, and is refused.
    ///
    /// Brought to the user as
    /// [`app_dialog_presentation`](crate::window_manager::app_dialog_presentation)
    /// decides: restored and in front when the user asked for it (their
    /// input, an activation token, a window of the application taking the
    /// focus, a quit waiting on its answer) or the window or a dialog of it
    /// has the focus;
    /// otherwise shown where it is while the application asks for
    /// attention; and owed, not shown, when it would open a window of its
    /// own over a minimised window. Owed dialogs come first, so the order
    /// they were asked in holds; once one is owed, the ones after it wait
    /// with it.
    fn process_app_tree_modal_requests(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let asked = self
            .wm
            .app_tree_mut()
            .map(|tree| tree.drain_pending_modal_requests())
            .unwrap_or_default();
        let owed = self.wm.take_owed_app_dialogs();
        let came_back = self.wm.take_owed_app_dialogs_due();
        if asked.is_empty() && owed.is_empty() {
            return false;
        }
        let already_owing = !owed.is_empty();
        let native_supported = native_modal_windows_supported();
        let for_user = self.wm.acts_for_user() || came_back || self.wm.quit_awaits_answer();
        self.wm.refresh_minimised_windows();
        let mut presented = false;
        let mut owing = false;
        for request in owed
            .into_iter()
            .chain(asked.into_iter().map(|queued| queued.request))
        {
            if matches!(request.content, ModalContent::ExistingWidget(_)) {
                eprintln!(
                    "teksilo-app: a modal request from the application tree presents an \
                     existing widget, which cannot move to a window's tree; nothing shown. \
                     Build the content with ModalRequest::deferred."
                );
                continue;
            }
            if owing {
                self.wm.owe_app_dialog(request);
                continue;
            }
            let Some(window) = self.wm.app_modal_target() else {
                self.open_modal_window(None, request, event_loop);
                presented = true;
                continue;
            };
            let resolved = resolve_modal_presentation(
                request.presentation,
                &request.content,
                native_supported,
            );
            let presentation = crate::window_manager::app_dialog_presentation(
                for_user,
                self.wm.knows_focused_with_dialogs(window),
                self.wm.knows_minimised(window),
                resolved == ResolvedModalPresentation::NativeWindow,
            );
            if presentation == crate::window_manager::AppDialogPresentation::Owe {
                // The attention asked as the first is owed lasts until the
                // user comes to the application.
                if !already_owing {
                    self.wm.show_for_app_tree_dialog(window, presentation);
                }
                owing = true;
                self.wm.owe_app_dialog(request);
                continue;
            }
            self.wm.show_for_app_tree_dialog(window, presentation);
            presented = true;
            match resolved {
                ResolvedModalPresentation::InTree => {
                    if let Some(managed) = self.wm.get_by_teksilo_mut(window)
                        && let Some(&anchor) = managed.tree.roots().first()
                    {
                        present_in_tree_modal_request(&mut managed.tree, anchor, request);
                        managed.request_redraw();
                    }
                }
                ResolvedModalPresentation::NativeWindow => {
                    self.wm.set_raise_next_modal(
                        presentation == crate::window_manager::AppDialogPresentation::Raise,
                    );
                    self.open_modal_window(Some(window), request, event_loop);
                    self.wm.set_raise_next_modal(false);
                }
            }
        }
        presented
    }

    fn process_modal_dismissals(&mut self) -> bool {
        let windows_to_close = self.wm.drain_pending_modal_dismissals();
        let had_dismissals = !windows_to_close.is_empty();

        for window_id in windows_to_close {
            self.wm.queue_close(window_id);
        }

        had_dismissals
    }

    fn maybe_exit(&self, event_loop: &ActiveEventLoop) {
        if self
            .quit_policy
            .exits_with(self.wm.window_count(), self.wm.quit_completed())
        {
            event_loop.exit();
        }
    }

    /// Run the non-visual frame of every window whose gate lets a pending
    /// tick run now: a window that draws nothing (hidden, or its redraw
    /// withheld), whose requests become these ticks since it gets no
    /// `RedrawRequested` to run them in, and a window that draws but was
    /// asked to bring state up to date without redrawing. See
    /// [`crate::redraw_gate`].
    ///
    /// A tick first hands the window the accessibility actions waiting for
    /// it, which would otherwise wait for a window event that does not come,
    /// and ends like any event, with [`post_event`](Self::post_event), so
    /// what the frame queued (a size-to-content resize, a close, a change
    /// another window displays) is acted on now rather than at the next
    /// unrelated event.
    fn run_ticks(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let due: Vec<WindowId> = self
            .wm
            .windows_map()
            .iter()
            .filter(|(_, managed)| managed.redraw.take_tick(now))
            .map(|(id, _)| *id)
            .collect();
        if due.is_empty() {
            return;
        }
        for window_id in due {
            self.drain_accessibility_actions(window_id, event_loop);
            let Some(mut current) = self.wm.take_managed(window_id) else {
                continue;
            };
            if let Some(trace) = &mut self.idle_trace {
                trace.note_tick();
            }
            self.run_nonvisual_frame(&mut current, event_loop);
            self.wm.reinsert_managed(window_id, current);
        }
        self.post_event(event_loop);
    }

    /// Report, once per stall, a focused window that has waited longer than
    /// [`STALL_REPORT_AFTER`](crate::redraw_gate::STALL_REPORT_AFTER) for a
    /// redraw it requested. An unfocused one is most likely a window its
    /// compositor does not show, which is expected to wait; a focused one is
    /// on screen, and it draws nothing until the redraw arrives.
    #[cfg(debug_assertions)]
    fn report_stalled_redraws(&self) {
        let now = Instant::now();
        for managed in self.wm.iter().filter(|managed| managed.focused) {
            if let Some(waited) = managed.redraw.take_stall(now) {
                eprintln!(
                    "teksilo-app: focused window {:?} has waited {waited:?} for a redraw it \
                     requested and draws nothing until it arrives",
                    managed.teksilo_id
                );
            }
        }
    }

    fn update_control_flow(&mut self, event_loop: &ActiveEventLoop) {
        // Tick time-driven gesture recognizers (long-press) on every tree
        // before computing the next deadline. Without this, a long-press
        // that expired between frames would never fire until the next
        // unrelated pointer event. Handlers that run may emit commands
        // and mark nodes dirty — request a redraw on those windows.
        let now = Instant::now();
        // Collect winit ids up front so we can safely iterate without
        // holding a borrow on `self.wm.windows` across the
        // `tick_gestures_in_window` calls (each of which briefly
        // takes a window out of the map).
        let winit_ids: Vec<_> = self.wm.windows_map().keys().copied().collect();
        for winit_id in winit_ids {
            let before = self
                .wm
                .get_by_winit_mut(winit_id)
                .map(|m| m.tree.has_idle_work())
                .unwrap_or(false);
            self.tick_gestures_in_window(winit_id, now, event_loop);
            if let Some(managed) = self.wm.get_by_winit_mut(winit_id)
                && managed.tree.has_idle_work() != before
            {
                managed.request_redraw();
            }
        }

        let mut earliest_deadline: Option<Instant> = None;
        let mut timer_windows = 0_usize;
        let mut animation_timers = 0_usize;
        let mut tooltip_timers = 0_usize;
        for managed in self.wm.iter() {
            let animation_count = managed.tree.active_animation_count();
            let tooltip_count = managed.tree.pending_tooltip_count();
            if animation_count > 0 || tooltip_count > 0 {
                timer_windows += 1;
            }
            animation_timers += animation_count;
            tooltip_timers += tooltip_count;
            // `next_timer_deadline` now folds in the per-frame-effect
            // path's fixed 60 Hz deadline (Pulse / Cycle / caret blink /
            // drag auto-scroll) alongside the tween + shader schedulers,
            // so continuous animations pace through `WaitUntil` below
            // instead of forcing `ControlFlow::Poll` (which free-ran at
            // the display's refresh rate — 300 fps on a 300 Hz panel).
            // Held back for a window awaiting a redraw it requested or
            // drawing nothing: see `crate::redraw_gate`.
            if let Some(deadline) = managed.wake_deadline(now) {
                earliest_deadline = Some(match earliest_deadline {
                    Some(current) => current.min(deadline),
                    None => deadline,
                });
            }
        }

        // The application tree's timers: its tweens, a requested wake, its
        // frame tick. Served by the tick in `about_to_wait`.
        if let Some(deadline) = self
            .wm
            .app_tree()
            .and_then(|tree| tree.next_unpainted_deadline())
        {
            earliest_deadline = Some(match earliest_deadline {
                Some(current) => current.min(deadline),
                None => deadline,
            });
        }

        // While nothing on screen can be seen, the session watch rereads the
        // system's state now and then (`WindowManager::recheck_session`).
        if let Some(recheck) = self.wm.session_deadline() {
            earliest_deadline = Some(match earliest_deadline {
                Some(current) => current.min(recheck),
                None => recheck,
            });
        }

        // The digitizer, whose packets arrive on a thread of its own and so
        // cannot wake the loop by themselves. A bounded `WaitUntil` term, not
        // a `Poll`: see `input_loop::pen_deadline` for when each of its two
        // terms applies and when neither does.
        if let Some(pen) = self.pen_deadline(now) {
            earliest_deadline = Some(match earliest_deadline {
                Some(current) => current.min(pen),
                None => pen,
            });
        }

        // Textures a renderer dropped (a live picture left a window, a window
        // closed) are freed once the GPU has finished the last submission
        // that used them, at a device maintenance only a submission or a poll
        // runs. A loop that draws nothing more submits nothing, so it polls
        // for them instead, waking every `GPU_RECLAIM_POLL` until they are
        // freed, normally within two frames. No frame is drawn for it.
        let reclaim_pending = (self.gpu_reclaim)();
        if reclaim_pending {
            let reclaim = now + GPU_RECLAIM_POLL;
            earliest_deadline = Some(match earliest_deadline {
                Some(current) => current.min(reclaim),
                None => reclaim,
            });
        }

        // The ONLY consumer that forces true `ControlFlow::Poll`: an installed
        // loop-tick owner (e.g. the `teksilo-async` executor) with runnable
        // work. Async task processing wants to run as fast as possible and is
        // not an animation, so it is deliberately *not* 60 Hz-capped. Every
        // per-frame *animation* effect paces through the `WaitUntil` deadline
        // instead.
        let force_poll = self.loop_tick_poll.as_ref().is_some_and(|poll| poll.get());

        if force_poll {
            event_loop.set_control_flow(ControlFlow::Poll);
        } else if let Some(deadline) = earliest_deadline {
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }

        if let Some(trace) = &mut self.idle_trace {
            trace.note_wake_totals(self.wm.iter().map(|managed| {
                let stats = managed.platform_window.live_wake_stats();
                (
                    managed.platform_window.window().id(),
                    stats.wakes,
                    stats.dropped_hidden,
                )
            }));
            trace.note_control_flow(
                earliest_deadline.is_some(),
                timer_windows,
                animation_timers,
                tooltip_timers,
                reclaim_pending,
            );
            #[cfg(feature = "live-image-timings")]
            trace.report_live(self.wm.iter().map(|managed| {
                (
                    managed.platform_window.window().id(),
                    &managed.platform_window,
                )
            }));
        }
    }

    /// Read the window's platform safe area and hand it to the tree.
    ///
    /// The take-aside; the reading and the mapping are
    /// [`crate::input_loop::refresh_safe_area`].
    fn refresh_safe_area(managed: &mut crate::window_manager::ManagedWindow) {
        let chrome = WindowChrome::new(&managed.platform_window, &managed.redraw, None);
        crate::input_loop::refresh_safe_area(&chrome, &mut managed.tree);
    }

    /// Re-read the on-screen keyboard's rectangle and report it to every
    /// window it covers.
    ///
    /// Whether this turn is the one that re-reads is
    /// [`crate::input_loop::osk_poll_due`]; what each window makes of the
    /// rectangle is [`crate::input_loop::refresh_occluded_band`].
    fn refresh_occluded_band(&mut self) {
        let now = Instant::now();
        if !crate::input_loop::osk_poll_due(
            teksilo_platform::soft_keyboard::support(),
            self.osk_polled_at,
            now,
            OSK_POLL_INTERVAL,
        ) {
            return;
        }
        self.osk_polled_at = Some(now);
        let screen = teksilo_platform::soft_keyboard::keyboard_screen_rect();
        for managed in self.wm.iter_mut() {
            let chrome = WindowChrome::new(&managed.platform_window, &managed.redraw, None);
            crate::input_loop::refresh_occluded_band(&chrome, &mut managed.tree, screen);
        }
    }

    /// Whether any window's pen shim fills its buffer from a thread of its own.
    ///
    /// The Wayland tablet listener does; the Windows `WM_POINTER` subclass does
    /// not — its packets are already in the buffer by the time the turn that
    /// carried the message reaches the pump. Only the first kind needs the
    /// loop to look a second time.
    fn any_pen_source_polls_off_thread(&self) -> bool {
        self.wm
            .iter()
            .any(|managed| managed.translation_state.pen_polls_off_thread())
    }

    /// When the pen shim next has to be looked at, if ever.
    ///
    /// The take-aside; the folding is [`crate::input_loop::pen_deadline`],
    /// which is where the two terms and their bounds are described.
    fn pen_deadline(&mut self, now: Instant) -> Option<Instant> {
        let windows: Vec<_> = self
            .wm
            .iter()
            .map(|managed| crate::input_loop::PenWindow {
                off_thread: managed.translation_state.pen_polls_off_thread(),
                in_proximity: managed.translation_state.pen_in_proximity(),
            })
            .collect();
        crate::input_loop::pen_deadline(now, windows.into_iter(), &mut self.pen_recheck_at)
    }

    fn post_event(&mut self, event_loop: &ActiveEventLoop) {
        // First, so what its frame asks for (an application-wide change, a
        // quit, a window) is drained below in this same turn.
        self.tick_app_tree(event_loop);
        let mut redraw_all = false;
        // What a pass carries out can owe another: a routed intent's handler
        // asks for a theme or a dialog, a close guard leaves an intent, a
        // mount action makes a request. Bounded, as a handler could keep
        // asking.
        for pass in 0..4 {
            if pass > 0 && !self.wm.owes_turn_work() {
                break;
            }
            // The application tree's mount actions, which its tick's rebuild
            // queues, before the drains that must see what they ask for.
            if self
                .wm
                .app_tree()
                .is_some_and(|tree| tree.has_pending_mount_actions())
            {
                self.run_in_app_tree(event_loop, |tree, ops| tree.run_mount_actions(ops));
            }
            // App-wide environment changes (theme / locale) raised by a
            // handler in one window fan out to every window's tree, marking
            // the non-originating windows dirty. Those windows never received
            // the triggering event, so they would otherwise stay
            // un-repainted: `request_redraw_all()` below (gated on these
            // flags) fixes that.
            let had_locale = self.wm.drain_pending_locale_requests();
            let had_theme = self.wm.drain_pending_theme_requests();
            let had_follow_system = self.wm.drain_pending_follow_system_requests();
            let had_text_scale = self.wm.drain_pending_text_scale_requests();
            let had_commands = self.wm.drain_close_window_requests();
            // The application menu's rows chosen in a window whose closure is
            // the application tree's, before the intents left unclaimed: a
            // row's intent may be one of them.
            self.run_application_menu_rows(event_loop);
            // What a dispatch left unclaimed goes to the other side after the
            // close flags are queued, so a window closing this turn is no
            // target, and before the modal and close processing, which then
            // see what its handlers asked for.
            self.forward_unclaimed_intents(event_loop);
            let had_modal_requests = self.process_modal_requests(event_loop)
                | self.process_app_tree_modal_requests(event_loop);
            let had_modal_dismissals = self.process_modal_dismissals();
            self.process_pending(event_loop);
            // Drain post-mount actions (e.g. a WebView opening its native
            // engine subview, which needs the OS parent handle only reachable
            // here).
            self.process_pending_mount_actions(event_loop);
            // Drain per-window command queues: app-side writes to
            // WindowState signals emitted WindowCommand values that the
            // registry routes through the per-window queue. Translate each
            // into the appropriate winit call.
            self.wm.drain_window_commands();
            redraw_all |= had_locale
                || had_theme
                || had_follow_system
                || had_text_scale
                || had_commands
                || had_modal_requests
                || had_modal_dismissals;
        }
        // The application tree may have declared another menu or registered
        // a shortcut, and a handler may have changed the menu: every window
        // holds the shortcuts its rows name, and the native menu bar shows
        // the menu as it is.
        self.wm.sync_application_menu();
        if redraw_all {
            if let Some(trace) = &mut self.idle_trace {
                trace.note_request_redraw_all();
            }
            self.wm.request_redraw_all();
        }
        // Targeted counterpart to the blanket call above: a handler may have
        // mutated an app-level `Signal` that sibling windows also read,
        // dirtying their trees without those windows ever seeing the
        // triggering event. See `WindowManager::request_redraw_needing_render`
        // for why this is filtered rather than another `request_redraw_all()`.
        let cross_window_redraws = self.wm.request_redraw_needing_render();
        if cross_window_redraws > 0
            && let Some(trace) = &mut self.idle_trace
        {
            trace.note_cross_window_redraw(cross_window_redraws);
        }
        // The routed event's activation token is good for this turn only,
        // and so is the user's input as the source of what was asked.
        self.wm.clear_dispatch_activation_token();
        self.wm.clear_user_activation();
        self.maybe_exit(event_loop);
        self.update_control_flow(event_loop);
    }

    /// Dispatch a widget event into the named window's `WidgetTree`
    /// with a real [`teksilo_core::WindowOps`] sink so handlers can
    /// synchronously `open_window`, `focus_window`, etc.
    ///
    /// Re-entry pattern: the current `ManagedWindow` is temporarily
    /// removed from `WindowManager::windows` before dispatch and put
    /// back afterwards. The removed tree is borrowed mutably for the
    /// handler run; the `WindowOpsImpl` holds `&mut WindowManager`
    /// (with the tree out of the way) plus `&ActiveEventLoop`. Opening
    /// a new window from a handler therefore goes straight into
    /// `wm.create_window` without borrow-checker conflicts.
    fn dispatch_in_window(
        &mut self,
        window_id: WindowId,
        event: WidgetEvent,
        event_loop: &ActiveEventLoop,
    ) {
        let Some(mut current) = self.wm.take_managed(window_id) else {
            return;
        };
        let current_id = current.teksilo_id;

        let current_arc = Some(current.platform_window.window_arc());

        {
            let mut ops = crate::window_manager::WindowOpsImpl::new(
                &mut self.wm,
                event_loop,
                current_id,
                current_arc,
            );
            current.tree.dispatch_event_with_ops(event, &mut ops);
        }

        Self::settle_ime(&mut current);
        self.wm.reinsert_managed(window_id, current);
    }

    /// Take the window aside and run `f` with its translator, its tree and a
    /// real [`WindowOps`](teksilo_core::WindowOps) sink all borrowed at once.
    ///
    /// The pointer path needs all three — the backend to translate, the tree
    /// to receive, the ops sink so a handler can still open a window — and the
    /// only way to hold them together is the same take-aside
    /// [`Self::dispatch_in_window`] uses. Returns `f`'s value, or `None` when
    /// the window is gone.
    fn with_input_in_window<R>(
        &mut self,
        window_id: WindowId,
        event_loop: &ActiveEventLoop,
        f: impl FnOnce(
            &mut WindowChrome<'_>,
            &mut teksilo_platform::TranslationState,
            &mut teksilo_core::WidgetTree,
            &mut crate::window_manager::WindowOpsImpl<'_>,
        ) -> R,
    ) -> Option<R> {
        let mut current = self.wm.take_managed(window_id)?;
        let current_id = current.teksilo_id;
        let current_arc = Some(current.platform_window.window_arc());

        let result = {
            let mut ops = crate::window_manager::WindowOpsImpl::new(
                &mut self.wm,
                event_loop,
                current_id,
                current_arc,
            );
            let mut chrome = WindowChrome::new(
                &current.platform_window,
                &current.redraw,
                self.idle_trace.as_mut(),
            );
            f(
                &mut chrome,
                &mut current.translation_state,
                &mut current.tree,
                &mut ops,
            )
        };

        Self::settle_ime(&mut current);
        self.wm.reinsert_managed(window_id, current);
        Some(result)
    }

    /// Route one pointer / gesture winit event into the named window.
    ///
    /// Nothing but the take-aside: the routing and the chores it owes are
    /// [`crate::input_loop::dispatch_input`], which a test can drive with a
    /// hand-written winit event, a bare tree and a recording chrome.
    fn dispatch_input_in_window(
        &mut self,
        window_id: WindowId,
        event: &WindowEvent,
        event_loop: &ActiveEventLoop,
    ) {
        self.with_input_in_window(window_id, event_loop, |chrome, backend, tree, ops| {
            crate::input_loop::dispatch_input(chrome, backend, tree, ops, event);
        });
    }

    /// Drain every window's pen shim.
    ///
    /// Once per event-loop turn, which is what
    /// [`TranslationState::poll_pen`](teksilo_platform::TranslationState::poll_pen)
    /// asks for. What it costs is decided by what
    /// [`create_pen_source`](teksilo_platform::create_pen_source) answered when
    /// the window was made, and **no platform's answer depends on a digitizer
    /// being attached**: the shim is installed whenever the platform has a pen
    /// *path* — a compositor advertising `zwp_tablet_manager_v2`, or a Win32
    /// window whose subclass installs — and on X11 and macOS there is no path,
    /// so the null source is answered and `has_pen_source()` is false for every
    /// window. Where no shim was installed this returns before it allocates;
    /// where one was, a walk and one empty poll per window per turn are paid
    /// even on a machine that has never seen a stylus. What that shim costs
    /// *between* those polls is its own business and is bounded there —
    /// `WaylandPenSource::poll_interval` stands its dispatch thread down to a
    /// quarter-second tick until the seat announces a tool.
    fn pump_pen_sources(&mut self, event_loop: &ActiveEventLoop) {
        let winit_ids: Vec<_> = self
            .wm
            .windows_map()
            .iter()
            .filter(|(_, managed)| managed.translation_state.has_pen_source())
            .map(|(id, _)| *id)
            .collect();
        if winit_ids.is_empty() {
            self.pen_recheck_at = None;
            return;
        }

        // An external wake with a shim installed means a packet may be one
        // shim-poll away. Arm a single catch-up look; a session that produces
        // more will keep the loop ticking through `pen_deadline` on its own.
        if self.woken_externally && self.any_pen_source_polls_off_thread() {
            self.pen_recheck_at = Some(Instant::now() + teksilo_platform::pen::PEN_POLL_INTERVAL);
        }
        for winit_id in winit_ids {
            self.with_input_in_window(winit_id, event_loop, |chrome, backend, tree, ops| {
                crate::input_loop::pump_pen(chrome, backend, tree, ops)
            });
        }
    }

    /// Flip a window's active state, revoking every live pointer at both the
    /// backend and the tree when it goes inactive.
    fn set_window_active_in_window(
        &mut self,
        window_id: WindowId,
        active: bool,
        reason: teksilo_core::pointer::CancelReason,
        event_loop: &ActiveEventLoop,
    ) {
        self.with_input_in_window(window_id, event_loop, |_chrome, backend, tree, ops| {
            crate::input_routing::set_window_active(backend, tree, ops, active, reason);
        });
    }

    /// Apply what the OS reported about a window's focus to that window
    /// alone: its flags, its active state and a redraw. Returns the window's
    /// id when it took focus, which also makes it the
    /// [most recent window](crate::WindowManager::most_recent_window).
    fn apply_os_focus(
        &mut self,
        window_id: WindowId,
        focused: bool,
        event_loop: &ActiveEventLoop,
    ) -> Option<crate::window_config::TeksiloWindowId> {
        let active = if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
            managed.focused = focused;
            managed.state.set_focused_from_os(focused);
            if focused {
                // A window that takes focus is on screen.
                managed.minimized = false;
                managed.sync_hidden();
            } else {
                // An X11 iconify sends only this: the next redraw
                // re-reads the placement without waiting.
                managed.minimized_probed_at = None;
            }
            Some(managed.focused && !managed.occluded)
        } else {
            None
        };
        // Every live pointer is revoked at both ends on the way out: the
        // user releases the button over whatever took focus, and this
        // window is never told. See `input_routing`.
        if let Some(active) = active {
            self.set_window_active_in_window(
                window_id,
                active,
                teksilo_core::pointer::CancelReason::WindowDeactivated,
                event_loop,
            );
        }
        let managed = self.wm.get_by_winit_mut(window_id)?;
        // Drive a redraw on every focus transition so the window-active
        // observers (caret hide/restore, selection desaturation,
        // DimWhenInactive) reach a paint pass promptly: the OS does not
        // reliably emit RedrawRequested on focus change across all
        // platforms.
        managed.request_redraw();
        let teksilo_id = managed.teksilo_id;
        if !focused {
            return None;
        }
        self.wm.note_window_focused(teksilo_id);
        self.wm.note_attention_answered(teksilo_id);
        Some(teksilo_id)
    }

    /// Apply a [`MenubarAction`](teksilo_core::window::MenubarAction)
    /// decision from a window-level menubar dispatcher. Takes the
    /// managed window aside the same way
    /// [`Self::dispatch_in_window`] does so the action runs with
    /// `WindowOps` wired up (focus changes need to repaint, etc.).
    /// What each action does is
    /// [`input_routing::apply_menubar_action`](crate::input_routing::apply_menubar_action).
    fn apply_menubar_action(
        &mut self,
        window_id: WindowId,
        action: teksilo_core::window::MenubarAction,
        event_loop: &ActiveEventLoop,
    ) {
        let Some(mut current) = self.wm.take_managed(window_id) else {
            return;
        };
        let current_id = current.teksilo_id;

        let current_arc = Some(current.platform_window.window_arc());

        // A collapsed (hamburger) MenuBar's action carries a `reveal`
        // closure, followed by a synchronous layout pass so the trigger
        // has bounds to anchor its menu under. Lay out at the same
        // proposal the redraw path uses.
        let proposal = {
            let size = current.platform_window.surface_size();
            let sf = current.platform_window.scale_factor() as f32;
            SizeProposal::exact(size.0 as f32 / sf, size.1 as f32 / sf)
        };

        {
            let mut ops = crate::window_manager::WindowOpsImpl::new(
                &mut self.wm,
                event_loop,
                current_id,
                current_arc,
            );
            crate::input_routing::apply_menubar_action(
                &mut current.tree,
                &mut ops,
                action,
                proposal,
            );
        }

        Self::settle_ime(&mut current);
        self.wm.reinsert_managed(window_id, current);
    }

    /// Bring the winit window's OS-IME state in line with the focused
    /// widget's descriptor, then apply any pending
    /// `EventContext::request_soft_keyboard`.
    ///
    /// The take-aside; the decision — including why a turn at an unmoved focus
    /// must push nothing at all, and why the request half can never reach
    /// `set_ime_allowed` — is [`crate::input_loop::settle_ime`]. The caret
    /// area is reported separately (and idempotently) by the focused widget
    /// via `WindowOps::set_ime_cursor_area`.
    fn settle_ime(managed: &mut crate::window_manager::ManagedWindow) {
        let mut chrome = WindowChrome::new(&managed.platform_window, &managed.redraw, None);
        crate::input_loop::settle_ime(
            &mut chrome,
            &mut managed.tree,
            crate::input_loop::host_soft_keyboard_support(),
            &mut managed.ime_purpose,
            &mut managed.ime_allowed,
        );
    }

    /// Map the core `ImePurpose` onto winit's enum at the platform boundary.
    fn map_ime_purpose(purpose: teksilo_core::ImePurpose) -> winit::window::ImePurpose {
        match purpose {
            teksilo_core::ImePurpose::Normal => winit::window::ImePurpose::Normal,
            teksilo_core::ImePurpose::Password => winit::window::ImePurpose::Password,
            teksilo_core::ImePurpose::Terminal => winit::window::ImePurpose::Terminal,
        }
    }

    /// Route a debug-bridge [`AutomationPayload`](crate::automation_bridge::AutomationPayload):
    /// resolve the target window, then run the op against the live tree
    /// (and, for screenshots, the live `PlatformWindow`). `list_windows` and
    /// `screenshot` are served here (they need the window manager / platform
    /// window); everything else goes through [`teksilo_automation::execute`]
    /// with a real `WindowOps`. The settle runs synchronously on this (the
    /// main) thread, never across a frame boundary.
    #[cfg(all(feature = "automation", debug_assertions))]
    fn try_route_automation_payload(
        &mut self,
        payload: Box<dyn std::any::Any + Send>,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), Box<dyn std::any::Any + Send>> {
        use teksilo_automation::dto::{AutomationOp, AutomationReply, WindowInfo, codes};

        let payload = *payload.downcast::<crate::automation_bridge::AutomationPayload>()?;

        // Resolve target window: explicit id, else the most recent, else
        // primary.
        let bid = match payload.window_id {
            Some(raw) => crate::window_config::TeksiloWindowId::new(raw),
            None => self
                .wm
                .most_recent_window()
                .unwrap_or_else(|| self.wm.primary_window_id()),
        };
        let Some(winit_id) = self.wm.winit_id_for_teksilo(bid) else {
            let _ = payload
                .reply_tx
                .send(AutomationReply::err(codes::NOT_FOUND, "no such window"));
            return Ok(());
        };

        // `list_windows` is served straight from the window manager.
        if matches!(payload.op, AutomationOp::ListWindows) {
            let windows: Vec<WindowInfo> = self
                .wm
                .iter()
                .map(|m| WindowInfo {
                    id: m.teksilo_id.raw(),
                    label: m.string_id.clone(),
                    title: Some(m.state.title().get()),
                    focused: m.focused,
                })
                .collect();
            let _ = payload.reply_tx.send(AutomationReply::ok_json(&windows));
            return Ok(());
        }

        // Screenshots reach the `ManagedWindow` (tree + platform window).
        if matches!(payload.op, AutomationOp::Screenshot { .. }) {
            self.automation_screenshot(winit_id, event_loop, &payload);
            return Ok(());
        }

        // A live picture's stats: the tree answers for the source and its
        // attachment, the window adds its renderer's textures and its wake
        // counters.
        if matches!(payload.op, AutomationOp::LiveImageStats { .. }) {
            let crate::automation_bridge::AutomationPayload {
                op,
                settle,
                reply_tx,
                ..
            } = payload;
            let settle = crate::automation_bridge::clamp_live_settle(&settle);
            let mut reply = None;
            self.run_in_window(winit_id, event_loop, |tree, ops| {
                reply = Some(teksilo_automation::execute(tree, ops, &op, &settle));
            });
            let reply = match (reply, self.wm.windows_map().get(&winit_id)) {
                (Some(reply), Some(managed)) => {
                    let wakes = managed.platform_window.live_wake_stats();
                    teksilo_automation::with_window_stats(
                        reply,
                        Some(managed.platform_window.live_texture_stats()),
                        Some(teksilo_automation::dto::LiveWakeStatsDto {
                            wakes: wakes.wakes,
                            wakes_dropped_hidden: wakes.dropped_hidden,
                            window_hidden: wakes.hidden,
                        }),
                    )
                }
                _ => AutomationReply::err(codes::NOT_FOUND, "window vanished"),
            };
            let _ = reply_tx.send(reply);
            return Ok(());
        }

        // Everything else: a per-tree op with a real `WindowOps`.
        let crate::automation_bridge::AutomationPayload {
            op,
            settle,
            reply_tx,
            ..
        } = payload;
        // Clamp the settle: this runs on the winit main thread, so an
        // unbounded wait/settle would freeze the live UI (see Risk 1).
        let settle = crate::automation_bridge::clamp_live_settle(&settle);
        self.run_in_window(winit_id, event_loop, move |tree, ops| {
            let reply = teksilo_automation::execute(tree, ops, &op, &settle);
            let _ = reply_tx.send(reply);
        });
        if let Some(m) = self.wm.windows_map().get(&winit_id) {
            m.request_redraw();
        }
        Ok(())
    }

    /// The screenshot arm of the automation bridge: take the window out of
    /// the manager (so we can borrow both its tree and its platform window),
    /// settle, render, capture offscreen, reinsert, then reply with a
    /// base64-PNG.
    #[cfg(all(feature = "automation", debug_assertions))]
    fn automation_screenshot(
        &mut self,
        winit_id: winit::window::WindowId,
        event_loop: &ActiveEventLoop,
        payload: &crate::automation_bridge::AutomationPayload,
    ) {
        use teksilo_automation::dto::{AutomationOp, AutomationReply, codes};

        let node = match &payload.op {
            AutomationOp::Screenshot { node } => *node,
            _ => None,
        };

        let Some(mut current) = self.wm.take_managed(winit_id) else {
            let _ = payload
                .reply_tx
                .send(AutomationReply::err(codes::NOT_FOUND, "window vanished"));
            return;
        };
        let current_id = current.teksilo_id;
        let current_arc = Some(current.platform_window.window_arc());

        // Settle synchronously on the main thread with a real `WindowOps`.
        {
            let mut ops = crate::window_manager::WindowOpsImpl::new(
                &mut self.wm,
                event_loop,
                current_id,
                current_arc,
            );
            let settle = crate::automation_bridge::clamp_live_settle(&payload.settle);
            let _ = teksilo_automation::run_settle(&mut current.tree, &mut ops, &settle);
        }

        // Optional crop rect in physical pixels (logical bounds × scale).
        let scale = current.tree.device_scale_factor();
        let crop = node.and_then(|n| {
            let nid = teksilo_core::accesskit::NodeId(n);
            let wid = teksilo_core::accessibility::node_id_to_widget_id_maybe(nid)
                .or_else(|| current.tree.widget_for_synthetic(nid))?;
            let b = current.tree.bounds(wid);
            Some(teksilo_canvas::Rect {
                x: b.x * scale,
                y: b.y * scale,
                width: b.width * scale,
                height: b.height * scale,
            })
        });

        // WebView blind-spot warning: a native engine view is composited
        // over the window by the OS, so the capture misses it. A page an
        // engine renders into the frame (`WebViewRendering::Surface`) is in
        // the capture like any other content.
        let warnings = {
            // `accessibility_tree_snapshot`, not `sync_accessibility`: this
            // update is inspected and dropped. Delivering it would consume one
            // step of the framework's live regions, so taking a screenshot
            // while something was being announced would eat the announcement.
            let update = current.tree.accessibility_tree_snapshot();
            let has_web_view = update
                .nodes
                .iter()
                .any(|(_, nd)| nd.role() == teksilo_core::accesskit::Role::WebView);
            #[cfg(feature = "web-view")]
            let drawn_in_frame = self
                .wm
                .app_context_template()
                .and_then(|t| t.app_state::<teksilo_webview::WebViewRegistry>().cloned())
                .is_some_and(|registry| {
                    matches!(
                        registry.rendering(),
                        teksilo_webview::WebViewRendering::Surface { .. }
                    )
                });
            #[cfg(not(feature = "web-view"))]
            let drawn_in_frame = false;
            web_view_hole_warning(has_web_view, drawn_in_frame)
        };

        let clear = teksilo_render::vertex::srgb_to_linear_rgba(
            current.tree.theme().colors.surface_main.to_array(),
        );
        let frame = current.tree.render();
        let frame = self.sync_window_glyph_atlas(&mut current, frame, event_loop);
        // A lost device fails the readback as a value. A panic anywhere in
        // the render (a wgpu validation error goes to its panicking default
        // handler) is caught too, so the window is still reinserted (no
        // zombie) and the app survives: a screenshot failure must not abort a
        // live session. A live picture's lock is released on unwind, and its
        // texture cursor moves only after its writes, so the next frame plans
        // its upload again.
        let captured = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            current
                .platform_window
                .capture_offscreen(&frame, clear, crop)
        }));

        // The live pictures the image shows, from what the capture's render
        // decided for each.
        let live_images = match &captured {
            Ok(Ok(shot)) => teksilo_automation::live_image_shots(
                &current.tree,
                &frame,
                current.platform_window.renderer().live_image_decisions(),
                scale,
                shot.region,
            ),
            _ => Vec::new(),
        };

        current.request_redraw();
        self.wm.reinsert_managed(winit_id, current);

        let reply = match captured {
            Ok(Ok(shot)) if shot.width != 0 && shot.height != 0 => {
                crate::automation_bridge::screenshot_reply(
                    &shot.rgba,
                    shot.width,
                    shot.height,
                    scale,
                    warnings,
                    live_images,
                )
            }
            Ok(Ok(_)) => {
                AutomationReply::err(codes::BAD_ARGUMENT, "crop region empty / outside window")
            }
            Ok(Err(error)) => AutomationReply::err(
                codes::GPU_READBACK_FAILED,
                format!("offscreen capture failed: {error}"),
            ),
            Err(_) => AutomationReply::err(
                codes::GPU_READBACK_FAILED,
                "offscreen capture failed (GPU device lost?)",
            ),
        };
        let _ = payload.reply_tx.send(reply);
    }

    /// Upload the glyph atlas `frame`'s text samples into `managed`'s
    /// renderer, and return the frame to draw.
    ///
    /// When glyphs were evicted since the last upload, every retained paint
    /// frame in every window may hold quads whose atlas UVs now point at
    /// recycled slots, and `invalidate_cache` also kills the `touch_layout`
    /// keep-alive for frames baked before the clear. So every window is
    /// invalidated, not just this one (the others re-render at their own
    /// redraw and pull the current atlas through the version comparison),
    /// and this one is rendered again before the atlas is uploaded once
    /// more. Shared by the redraw path and automation screenshots: both draw
    /// through this window's renderer, and a capture that skipped the upload
    /// drew any glyph first rasterised by its own render as a blank.
    fn sync_window_glyph_atlas(
        &mut self,
        managed: &mut crate::window_manager::ManagedWindow,
        frame: std::rc::Rc<teksilo_canvas::RenderFrame>,
        event_loop: &ActiveEventLoop,
    ) -> std::rc::Rc<teksilo_canvas::RenderFrame> {
        let evicted = sync_glyph_atlas(
            &self.typesetter,
            managed.platform_window.renderer_mut(),
            &mut managed.atlas_uploaded_version,
        );
        if !evicted {
            return frame;
        }
        self.typesetter.bridge().borrow_mut().invalidate_cache();
        managed.tree.invalidate_all_paints();
        for other in self.wm.iter_mut() {
            other.tree.invalidate_all_paints();
            other.request_redraw();
        }
        // Re-render with a real ops sink so rebuild-triggered handlers on
        // this recovery path can still open windows.
        let current_id = managed.teksilo_id;
        let current_arc = Some(managed.platform_window.window_arc());
        let mut ops = crate::window_manager::WindowOpsImpl::new(
            &mut self.wm,
            event_loop,
            current_id,
            current_arc,
        );
        let frame = managed.tree.render_with_ops(&mut ops);
        let evicted_again = sync_glyph_atlas(
            &self.typesetter,
            managed.platform_window.renderer_mut(),
            &mut managed.atlas_uploaded_version,
        );
        // The recovery re-render cannot legitimately evict again (the
        // eviction scan's generation-cadence gate just reset), but
        // atlas_info consumes the epoch delta, so a report here would be
        // silently lost: check the assumption instead of assuming it.
        debug_assert!(
            !evicted_again,
            "glyph eviction during eviction recovery — epoch delta would be lost"
        );
        frame
    }

    /// Run `f` against window `winit_id`'s tree with a real
    /// [`WindowOps`](teksilo_core::WindowOps) sink (so `open_window`,
    /// `parent_window_handle`, etc. work). Encapsulates the take-out /
    /// build-`WindowOpsImpl` / reinsert dance that the `AppEvent::External`
    /// routers and the mount-action drain all share — keeping the reinsert
    /// (whose omission silently freezes a window) in exactly one place.
    /// No-op if `winit_id` is not a managed window.
    fn run_in_window(
        &mut self,
        winit_id: winit::window::WindowId,
        event_loop: &ActiveEventLoop,
        f: impl FnOnce(&mut WidgetTree, &mut crate::window_manager::WindowOpsImpl),
    ) {
        let Some(mut current) = self.wm.take_managed(winit_id) else {
            return;
        };
        let current_id = current.teksilo_id;

        let current_arc = Some(current.platform_window.window_arc());

        {
            let mut ops = crate::window_manager::WindowOpsImpl::new(
                &mut self.wm,
                event_loop,
                current_id,
                current_arc,
            );
            f(&mut current.tree, &mut ops);
        }

        self.wm.reinsert_managed(winit_id, current);
    }

    /// Last stop for an `AppEvent::External` payload: hand it to the app's own
    /// [`ExternalCtxHandler`] (if one was registered) with a live
    /// [`EventContext`](teksilo_core::widget::EventContext), so it can open,
    /// find and focus windows.
    ///
    /// **Target window** = the
    /// [most recent](crate::WindowManager::most_recent_window) one, else the
    /// primary, the same resolution
    /// [`try_route_automation_payload`](Self::try_route_automation_payload)
    /// uses. The handler is about *application*-level intent ("open this
    /// document"), so which window hosts the context is an implementation
    /// detail; it just has to be a real one, because `open_window` on a
    /// standalone context panics.
    ///
    /// The handler is `take`n for the duration of the call and put back
    /// afterwards: [`run_in_window`](Self::run_in_window) needs `&mut self`, and
    /// the handler lives on `self`. Re-entrancy (a handler whose body somehow
    /// pumps another external event) therefore sees `None` and is a no-op rather
    /// than a double borrow.
    ///
    /// With no window open, the handler runs in the application tree
    /// instead, with a context whose `window()` is `None`: still able to open
    /// a window, which is what a single-instance application with none open
    /// (a tray application, an application under
    /// [`QuitPolicy::Explicit`]) needs.
    fn route_external_with_ctx(
        &mut self,
        payload: &(dyn std::any::Any + Send),
        event_loop: &ActiveEventLoop,
    ) {
        let Some(mut handler) = self.external_ctx_handler.take() else {
            return;
        };
        // A payload that came with an activation token: the handler sees
        // what it wraps, and the token waits for its first raise or open.
        // `user_event` unwraps it before any handler; this catches one
        // handed here directly.
        let payload = match payload.downcast_ref::<WithActivationToken>() {
            Some(wrapped) => {
                self.wm.set_dispatch_activation_token(wrapped.token.clone());
                wrapped.payload()
            }
            None => payload,
        };
        // "No window open" asked of the window map itself, which is what
        // the application tree stands in for.
        let target = if self.wm.is_empty() {
            None
        } else {
            self.wm
                .most_recent_window()
                .and_then(|id| self.wm.winit_id_for_teksilo(id))
        };
        let handler_ref = &mut handler;
        match target {
            Some(winit_id) => self.run_in_window(winit_id, event_loop, move |tree, ops| {
                tree.run_with_event_context(ops, |ctx| {
                    handler_ref(payload, ctx);
                });
            }),
            None => self.run_in_app_tree(event_loop, move |tree, ops| {
                tree.run_with_event_context(ops, |ctx| {
                    handler_ref(payload, ctx);
                });
            }),
        }
        self.wm.end_dispatch_activation();
        self.external_ctx_handler = Some(handler);
    }

    /// Try to route an `AppEvent::External` payload as a
    /// [`FileDialogEventPayload`](teksilo_platform::file_dialog::FileDialogEventPayload).
    /// Returns `Ok(())` if the payload matched and was delivered to
    /// the originating window's tree, `Err(payload)` to hand the
    /// box back for fallthrough to other downcast attempts.
    ///
    /// Routing details:
    /// - Resolves `payload.window_id_owner` to the matching winit
    ///   `WindowId` via `WindowManager::teksilo_to_winit_map`.
    /// - Temporarily takes the window out of `WindowManager::windows`
    ///   (matches the `dispatch_in_window` re-entry pattern) so
    ///   `open_window` / other ops calls inside the result callback
    ///   can run.
    /// - Builds a `WidgetTree::run_with_event_context` closure that
    ///   pops the pending callback from `FileDialogHandle` and
    ///   invokes it.
    /// - On any miss (no matching window, no handle in app-state,
    ///   already-purged callback) the result is silently dropped —
    ///   no panic, no leaked callback.
    #[cfg_attr(not(feature = "file-dialog"), allow(unused_variables))]
    fn try_route_file_dialog_payload(
        &mut self,
        payload: Box<dyn std::any::Any + Send>,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), Box<dyn std::any::Any + Send>> {
        #[cfg(feature = "file-dialog")]
        {
            use teksilo_platform::file_dialog::{FileDialogEventPayload, FileDialogHandle};

            let payload = *payload.downcast::<FileDialogEventPayload>()?;

            // Find the originating window.
            let target_winit = self
                .wm
                .teksilo_to_winit_map()
                .get(&payload.window_id_owner)
                .copied();
            let Some(winit_id) = target_winit else {
                // Window already torn down — drop silently.
                return Ok(());
            };

            // Pull the FileDialogHandle out of the shared app context
            // template. Same Rc held by every window's tree, so this
            // does not fight take_managed below.
            let handle = self
                .wm
                .app_context_template()
                .and_then(|t| t.app_state::<FileDialogHandle>().cloned());
            let Some(handle) = handle else {
                // Application did not install a FileDialogHandle —
                // shouldn't happen if a payload was dispatched, but
                // drop silently rather than panic.
                return Ok(());
            };

            let Some(mut current) = self.wm.take_managed(winit_id) else {
                return Ok(());
            };
            let current_id = current.teksilo_id;

            let current_arc = Some(current.platform_window.window_arc());

            {
                let mut ops = crate::window_manager::WindowOpsImpl::new(
                    &mut self.wm,
                    event_loop,
                    current_id,
                    current_arc,
                );
                current
                    .tree
                    .run_with_event_context(&mut ops, |ctx| handle.deliver(payload, ctx));
            }

            self.wm.reinsert_managed(winit_id, current);
            Ok(())
        }
        #[cfg(not(feature = "file-dialog"))]
        {
            Err(payload)
        }
    }

    /// Drain queued post-mount actions for every window that has any, each
    /// with a real [`EventContext`](teksilo_core::widget::EventContext) (so `ctx.parent_window_handle()` resolves).
    /// Modal-blocked windows are skipped — their actions (e.g. a WebView
    /// opening its native engine subview) stay queued until the modal closes,
    /// so a native surface can't appear over a modal. Cheap when nothing is
    /// queued (the common case): one map scan, the returned Vec is empty and
    /// unallocated.
    fn process_pending_mount_actions(&mut self, event_loop: &ActiveEventLoop) {
        let winit_ids = self.wm.winit_ids_with_pending_mount_actions();
        for winit_id in winit_ids {
            self.run_in_window(winit_id, event_loop, |tree, ops| {
                tree.run_mount_actions(ops)
            });
        }
        if self
            .wm
            .app_tree()
            .is_some_and(|tree| tree.has_pending_mount_actions())
        {
            self.run_in_app_tree(event_loop, |tree, ops| tree.run_mount_actions(ops));
        }
    }

    /// Run a logic frame of the application tree, when it has one to run
    /// ([`needs_logic_frame`](teksilo_core::WidgetTree::needs_logic_frame)):
    /// its idle callbacks, then layout, which runs its tweens, bindings and
    /// frame tick. Never gated on `needs_render`, which a tree that is never
    /// painted always reports.
    fn tick_app_tree(&mut self, event_loop: &ActiveEventLoop) {
        // Gated on what the tree has to do, so a frame request in it paces
        // at its deadline rather than at the rate events arrive.
        if !self
            .wm
            .app_tree()
            .is_some_and(|tree| tree.needs_logic_frame(Instant::now()))
        {
            return;
        }
        let budget = self.idle_budget;
        self.run_in_app_tree(event_loop, |tree, ops| {
            run_tree_logic_frame(tree, budget, SizeProposal::exact(0.0, 0.0), ops);
        });
        #[cfg(debug_assertions)]
        if !self.warned_visual_app_tree
            && let Some(tree) = self.wm.app_tree()
        {
            let what = if tree.animated_quad_count() > 0 {
                Some("an animated widget (a spinner, an indeterminate progress bar)")
            } else if !tree.overlay_manager().is_empty() {
                Some("an overlay (a popover, a tooltip, a menu)")
            } else {
                None
            };
            if let Some(what) = what {
                self.warned_visual_app_tree = true;
                eprintln!(
                    "teksilo-app: the application tree holds {what}, which is never drawn \
                     there. Show it from a window; a modal request from the application tree \
                     is presented in a window for you."
                );
            }
        }
    }

    /// Offer what each tree left unclaimed to the other side: the windows'
    /// to the application tree, the application tree's to the window the
    /// user works with ([`app_modal_target`](WindowManager::app_modal_target)),
    /// and to that one window only, as AppKit offers what the application
    /// leaves to its key window alone. Repeats while a routed intent's handler sends new
    /// ones across, for 16 rounds in a turn; the trees stop keeping a chain
    /// of intents after 64 crossings in all. See [`crate::intent_routing`].
    fn forward_unclaimed_intents(&mut self, event_loop: &ActiveEventLoop) {
        use crate::intent_routing::offer;
        for _ in 0..16 {
            let from_windows = self.wm.take_unclaimed_window_intents();
            let from_windows = self.offer_to_dialog_owners(from_windows, event_loop);
            let from_app = self
                .wm
                .app_tree_mut()
                .map(|tree| tree.take_unclaimed_intents())
                .unwrap_or_default();
            if from_windows.is_empty() && from_app.is_empty() {
                return;
            }
            if !from_windows.is_empty() {
                let mut dropped = Vec::new();
                if self.wm.app_tree().is_some() {
                    self.run_in_app_tree(event_loop, |tree, ops| {
                        dropped = offer(from_windows, Some(tree), ops);
                    });
                } else {
                    dropped = from_windows.iter().map(|intent| intent.name).collect();
                }
                self.dropped_intents.report("a window", dropped);
            }
            if !from_app.is_empty() {
                let target = self
                    .wm
                    .app_modal_target()
                    .and_then(|id| self.wm.winit_id_for_teksilo(id));
                let mut dropped: Vec<&'static str> =
                    from_app.iter().map(|intent| intent.name).collect();
                if let Some(winit_id) = target {
                    self.run_in_window(winit_id, event_loop, |tree, ops| {
                        dropped = offer(from_app, Some(tree), ops);
                    });
                }
                self.dropped_intents.report("the application tree", dropped);
            }
        }
        #[cfg(debug_assertions)]
        if self
            .wm
            .iter()
            .any(|managed| managed.tree.has_unclaimed_intents())
            || self
                .wm
                .app_tree()
                .is_some_and(|tree| tree.has_unclaimed_intents())
        {
            eprintln!(
                "teksilo-app: the windows and the application tree kept sending each other \
                 intents for 16 rounds; the rest wait for the next turn, and a chain of them \
                 stops after 64 crossings"
            );
        }
    }

    /// Offer each intent a native modal dialog window left unclaimed to the
    /// window it blocks, then to that window's own owner and so on up, as
    /// the chain was when the intent was taken (a window in between closed
    /// since is passed over, not the end of the climb), and return the
    /// intents none of them took, for the application tree. An in-tree
    /// dialog's intent walks on to the page in the same way. A window
    /// closing this turn is passed over. Each offer is
    /// [routed](WidgetTree::dispatch_routed_intent), so a window that does
    /// not take it does not keep it.
    ///
    /// An intent that stays in a modal
    /// ([`stays_in_modal`](teksilo_core::Intent::stays_in_modal): a key's, a
    /// native menu row's) is input to the dialog, and is offered to none of
    /// the windows it blocks: it goes to the application tree only, as an
    /// in-window dialog keeps it from the page.
    fn offer_to_dialog_owners(
        &mut self,
        intents: Vec<(teksilo_core::Intent, Vec<TeksiloWindowId>)>,
        event_loop: &ActiveEventLoop,
    ) -> Vec<teksilo_core::Intent> {
        let mut left = Vec::new();
        for (intent, owners) in intents {
            let owners = if intent.stays_in_modal() {
                Vec::new()
            } else {
                owners
            };
            let mut taken = false;
            for up in owners {
                if self.wm.is_closing(up) {
                    continue;
                }
                let Some(winit_id) = self.wm.winit_id_for_teksilo(up) else {
                    continue;
                };
                let offered = intent.clone();
                self.run_in_window(winit_id, event_loop, |tree, ops| {
                    taken = tree.dispatch_routed_intent(offered, ops);
                });
                if taken {
                    break;
                }
            }
            if !taken {
                left.push(intent);
            }
        }
        left
    }

    /// Run `f` against the application tree with a
    /// [windowless](crate::window_manager::WindowOpsImpl::windowless) ops
    /// sink, the tree taken out of the manager meanwhile as a window's is.
    /// A no-op when no application tree is installed.
    fn run_in_app_tree(
        &mut self,
        event_loop: &ActiveEventLoop,
        f: impl FnOnce(&mut WidgetTree, &mut crate::window_manager::WindowOpsImpl),
    ) {
        let Some(mut tree) = self.wm.take_app_tree() else {
            return;
        };
        {
            let mut ops =
                crate::window_manager::WindowOpsImpl::windowless(&mut self.wm, event_loop);
            f(&mut tree, &mut ops);
        }
        self.wm.reinsert_app_tree(tree);
    }

    /// Let the installed web-view engine run, on this thread: the answer to a
    /// [`WebViewPumpPayload`](teksilo_webview::WebViewPumpPayload). It belongs
    /// to no window: its frames reach their windows through the pictures they
    /// draw.
    #[cfg(feature = "web-view")]
    fn pump_web_views(&self) {
        let registry = self
            .wm
            .app_context_template()
            .and_then(|t| t.app_state::<teksilo_webview::WebViewRegistry>().cloned());
        if let Some(registry) = registry {
            registry.pump();
        }
    }

    /// Try to route an `AppEvent::External` payload as a
    /// [`WebViewEventPayload`](teksilo_webview::WebViewEventPayload) posted by a
    /// web-view engine backend, delivering it to the originating window's tree
    /// via [`WebViewRegistry::deliver`](teksilo_webview::WebViewRegistry::deliver).
    /// (A [`WebViewPumpPayload`](teksilo_webview::WebViewPumpPayload) never
    /// gets here: `user_event` answers it first, with `pump_web_views`.)
    /// Returns `Ok(())` if matched and delivered, `Err(payload)` to hand the
    /// box back for fallthrough. Same take/run-with-context/reinsert dance as
    /// [`Self::try_route_file_dialog_payload`].
    #[cfg_attr(not(feature = "web-view"), allow(unused_variables))]
    fn try_route_web_view_payload(
        &mut self,
        payload: Box<dyn std::any::Any + Send>,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), Box<dyn std::any::Any + Send>> {
        #[cfg(feature = "web-view")]
        {
            use teksilo_webview::{WebViewEventPayload, WebViewRegistry};

            let registry = || {
                self.wm
                    .app_context_template()
                    .and_then(|t| t.app_state::<WebViewRegistry>().cloned())
            };

            let payload = *payload.downcast::<WebViewEventPayload>()?;

            let target_winit = self
                .wm
                .teksilo_to_winit_map()
                .get(&payload.window_id_owner)
                .copied();
            let Some(winit_id) = target_winit else {
                return Ok(());
            };

            let Some(registry) = registry() else {
                return Ok(());
            };

            self.run_in_window(winit_id, event_loop, move |tree, ops| {
                tree.run_with_event_context(ops, |ctx| registry.deliver(payload, ctx));
            });
            Ok(())
        }
        #[cfg(not(feature = "web-view"))]
        {
            Err(payload)
        }
    }

    /// Try to route an `AppEvent::External` payload as an
    /// [`AsyncCompletionPayload`](teksilo_core::AsyncCompletionPayload) posted
    /// by the `teksilo-async` executor when a `spawn_local_with` future
    /// resolves. Returns `Ok(())` if matched and delivered, `Err(payload)` to
    /// hand the box back for fallthrough.
    ///
    /// Uses only teksilo-core types ([`AsyncCompletionHandle`](teksilo_core::AsyncCompletionHandle)),
    /// so `teksilo-async` (which depends on `teksilo-app`) never has to be a
    /// dependency here — the same take/run-with-context/reinsert pattern as
    /// the file-dialog path. On any miss (window gone, runtime not installed,
    /// already-purged completion) the result is dropped silently.
    fn try_route_async_completion_payload(
        &mut self,
        payload: Box<dyn std::any::Any + Send>,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), Box<dyn std::any::Any + Send>> {
        use teksilo_core::{AsyncCompletionHandle, AsyncCompletionPayload};

        let payload = *payload.downcast::<AsyncCompletionPayload>()?;

        let target_winit = self
            .wm
            .teksilo_to_winit_map()
            .get(&payload.window_id)
            .copied();
        let Some(winit_id) = target_winit else {
            // Window already torn down — drop silently.
            return Ok(());
        };

        let handle = self
            .wm
            .app_context_template()
            .and_then(|t| t.app_state::<AsyncCompletionHandle>().cloned());
        let Some(handle) = handle else {
            // No async runtime installed — drop silently.
            return Ok(());
        };

        let Some(mut current) = self.wm.take_managed(winit_id) else {
            return Ok(());
        };
        let current_id = current.teksilo_id;

        let current_arc = Some(current.platform_window.window_arc());

        {
            let mut ops = crate::window_manager::WindowOpsImpl::new(
                &mut self.wm,
                event_loop,
                current_id,
                current_arc,
            );
            current.tree.run_with_event_context(&mut ops, |ctx| {
                handle.deliver(payload.id, payload.window_id, ctx)
            });
        }

        self.wm.reinsert_managed(winit_id, current);
        Ok(())
    }

    /// Deliver a backend `AppEvent::SubscriptionEvent` to a *context-bearing*
    /// subscription registered via
    /// [`BuildContext::subscribe_event_with_ctx`](teksilo_core::BuildContext::subscribe_event_with_ctx):
    /// mint a fresh [`EventContext`](teksilo_core::EventContext) from the
    /// subscriber's window tree and invoke the stored callback inside it.
    ///
    /// Returns `true` iff `sub_id` names a context-bearing subscription — the
    /// caller then skips the plain, context-free dispatch (a `sub_id` lives in
    /// exactly one callback map). A `true` return with the window torn down (or
    /// mid-teardown) drops the event, exactly like the async-completion path;
    /// it still returns `true` so the stale event never falls through to the
    /// plain map.
    ///
    /// Mirrors [`try_route_async_completion_payload`](Self::try_route_async_completion_payload)'s
    /// take / run-with-context / reinsert dance — the one supported way to run
    /// application code with a fresh `EventContext` from the event loop.
    fn try_dispatch_subscription_with_ctx(
        &mut self,
        sub_id: SubscriptionId,
        event: &dyn std::any::Any,
        event_loop: &ActiveEventLoop,
    ) -> bool {
        let Some(template) = self.wm.app_context_template().cloned() else {
            return false;
        };
        let Some(target) = template.ctx_subscription_target(sub_id) else {
            return false;
        };
        // From here `sub_id` IS a context-bearing subscription: consume it
        // (return `true`) even if the window is gone, so a late event never
        // falls back to the plain, context-free map.
        let Some(window_id) = target else {
            // Registered in the application tree, which no window hosts.
            self.run_in_app_tree(event_loop, |tree, ops| {
                tree.run_with_event_context(ops, |ctx| {
                    template.dispatch_subscription_event_with_ctx(sub_id, event, ctx);
                });
            });
            return true;
        };
        let Some(winit_id) = self.wm.teksilo_to_winit_map().get(&window_id).copied() else {
            return true;
        };
        let Some(mut current) = self.wm.take_managed(winit_id) else {
            return true;
        };
        let current_id = current.teksilo_id;

        let current_arc = Some(current.platform_window.window_arc());

        {
            let mut ops = crate::window_manager::WindowOpsImpl::new(
                &mut self.wm,
                event_loop,
                current_id,
                current_arc,
            );
            current.tree.run_with_event_context(&mut ops, |ctx| {
                template.dispatch_subscription_event_with_ctx(sub_id, event, ctx);
            });
        }

        self.wm.reinsert_managed(winit_id, current);
        true
    }

    /// Try to route an `AppEvent::External` payload as a
    /// [`NativeMenuEventPayload`](teksilo_platform::native_menu::NativeMenuEventPayload)
    /// posted when the user chose an item in the platform's native menu bar.
    /// Resolves the item's [`MenuItemId`](teksilo_core::MenuItemId) to its
    /// recorded intent / action via the [`NativeMenuHandle`](teksilo_platform::native_menu::NativeMenuHandle)
    /// and fires it with `IntentSource::Menu`, as
    /// [`native_menu_route`](crate::native_menu_route) describes: the action in
    /// the window whose menu it is, the intent from the focused widget of the
    /// window with the OS focus. Any miss is dropped silently. A
    /// [`NativeAppMenuEventPayload`](teksilo_platform::native_menu::NativeAppMenuEventPayload),
    /// from the menu shown while no window's menu is, goes to
    /// [`Self::route_app_menu_item`].
    fn try_route_native_menu_payload(
        &mut self,
        payload: Box<dyn std::any::Any + Send>,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), Box<dyn std::any::Any + Send>> {
        use crate::native_menu_route::{fire_native_menu_item, native_menu_intent_window};
        use teksilo_platform::native_menu::{
            NativeAppMenuEventPayload, NativeMenuEventPayload, NativeMenuHandle,
        };

        // A native menu row the user chose: what its handlers ask is the
        // user's request, as a click's is.
        if payload.is::<NativeMenuEventPayload>() || payload.is::<NativeAppMenuEventPayload>() {
            self.wm.note_user_activation();
        }
        let payload = match payload.downcast::<NativeMenuEventPayload>() {
            Ok(payload) => *payload,
            Err(payload) => {
                let payload = payload.downcast::<NativeAppMenuEventPayload>()?;
                self.route_app_menu_item(payload.item_id, event_loop);
                return Ok(());
            }
        };

        let owner = payload.window_id_owner;
        let handle = self
            .wm
            .app_context_template()
            .and_then(|t| t.app_state::<NativeMenuHandle>().cloned());
        let Some(handle) = handle else {
            return Ok(());
        };
        let Some(activation) = handle.activation(owner, payload.item_id) else {
            // Item not found (menu replaced / window torn down) — drop.
            return Ok(());
        };
        if self.quit_instead(&activation, Some(owner)) {
            return Ok(());
        }
        let winit_of = |wm: &crate::window_manager::WindowManager, id| {
            wm.teksilo_to_winit_map().get(&id).copied()
        };
        let Some(owner_winit) = winit_of(&self.wm, owner) else {
            return Ok(());
        };
        let target = native_menu_intent_window(owner, self.wm.focused_window());
        let target_winit = winit_of(&self.wm, target).unwrap_or(owner_winit);

        if target_winit == owner_winit {
            self.run_in_window(owner_winit, event_loop, |tree, ops| {
                fire_native_menu_item(tree, ops, activation.action.as_ref(), activation.intent);
            });
        } else {
            // The action first, in the tree that holds its state, so the
            // intent's actions read what it wrote, as they do in one tree.
            if let Some(action) = activation.action.as_ref() {
                self.run_in_window(owner_winit, event_loop, |tree, ops| {
                    fire_native_menu_item(tree, ops, Some(action), None);
                });
            }
            self.run_in_window(target_winit, event_loop, |tree, ops| {
                fire_native_menu_item(tree, ops, None, activation.intent);
            });
        }
        Ok(())
    }

    /// Run the rows of the application menu chosen in a window whose
    /// closure belongs to the application tree, oldest first: each closure
    /// in the application tree, then its row's intent from the focus of the
    /// window it was chosen in, or in the application tree when that window
    /// has closed. With no application tree the closure runs in that window.
    fn run_application_menu_rows(&mut self, event_loop: &ActiveEventLoop) {
        use crate::native_menu_route::fire_window_menu_row;

        let Some(menu) = self.wm.application_menu() else {
            return;
        };
        for row in menu.take_pending() {
            let window = row.window.and_then(|id| self.wm.winit_id_for_teksilo(id));
            if let Some(action) = row.action.as_ref() {
                if self.wm.app_tree().is_some() {
                    self.run_in_app_tree(event_loop, |tree, ops| {
                        fire_window_menu_row(tree, ops, Some(action), None);
                    });
                } else if let Some(winit_id) = window {
                    self.run_in_window(winit_id, event_loop, |tree, ops| {
                        fire_window_menu_row(tree, ops, Some(action), None);
                    });
                }
            }
            if row.intent.is_some() {
                match window {
                    Some(winit_id) => self.run_in_window(winit_id, event_loop, |tree, ops| {
                        fire_window_menu_row(tree, ops, None, row.intent);
                    }),
                    None => self.run_in_app_tree(event_loop, |tree, ops| {
                        fire_window_menu_row(tree, ops, None, row.intent);
                    }),
                }
            }
        }
    }

    /// Fire an item chosen in the native application menu, the menu shown
    /// whenever the key window has no menu of its own and while no window is
    /// open: its action in the application tree, where the state it captured
    /// lives, then its intent from the focus of the window with the OS
    /// focus, or in the application tree when no window has it, as an
    /// in-window row of the application menu does. The App menu's Quit asks
    /// the application to quit when nothing takes its intent, or it has none
    /// ([`Self::quit_instead`]).
    fn route_app_menu_item(
        &mut self,
        item: teksilo_core::MenuItemId,
        event_loop: &ActiveEventLoop,
    ) {
        use crate::native_menu_route::fire_native_menu_item;
        use teksilo_platform::native_menu::NativeMenuHandle;

        let Some(handle) = self
            .wm
            .app_context_template()
            .and_then(|t| t.app_state::<NativeMenuHandle>().cloned())
        else {
            return;
        };
        let Some(activation) = handle.app_activation(item) else {
            return;
        };
        if self.quit_instead(&activation, None) {
            return;
        }
        if let Some(action) = activation.action.as_ref() {
            self.run_in_app_tree(event_loop, |tree, ops| {
                fire_native_menu_item(tree, ops, Some(action), None);
            });
        }
        if activation.intent.is_none() {
            return;
        }
        match self
            .wm
            .focused_window()
            .and_then(|id| self.wm.winit_id_for_teksilo(id))
        {
            Some(winit_id) => self.run_in_window(winit_id, event_loop, |tree, ops| {
                fire_native_menu_item(tree, ops, None, activation.intent);
            }),
            None => self.run_in_app_tree(event_loop, |tree, ops| {
                fire_native_menu_item(tree, ops, None, activation.intent);
            }),
        }
    }

    /// For the App menu's Quit row (`activation.quits`): ask the application
    /// to quit, through every window's close guard, when its intent is
    /// missing or nothing on the native menu's route would take it, and
    /// say so. A row that is not Quit, or whose intent something takes,
    /// answers `false` and is fired as any row is. `owner` is the window
    /// whose own menu the row is in, `None` for the application menu.
    fn quit_instead(
        &mut self,
        activation: &teksilo_platform::native_menu::NativeMenuActivation,
        owner: Option<crate::TeksiloWindowId>,
    ) -> bool {
        if !activation.quits
            || activation
                .intent
                .is_some_and(|name| self.wm.native_menu_command_handled(name, owner))
        {
            return false;
        }
        self.wm.quit();
        true
    }

    /// Try to interpret an `AppEvent::External` payload as an
    /// [`ExternalDndEventPayload`](teksilo_platform::external_dnd::ExternalDndEventPayload)
    /// posted by a platform drag backend and route it to the originating
    /// window's tree, driving the matching `*_external_drag` method.
    ///
    /// Returns `Ok(())` if the payload was an external-drag event (consumed),
    /// or `Err(payload)` to hand it back for other downcast attempts. Mirrors
    /// [`Self::try_route_file_dialog_payload`]'s take/dispatch/reinsert dance.
    fn try_route_external_dnd_payload(
        &mut self,
        payload: Box<dyn std::any::Any + Send>,
        event_loop: &ActiveEventLoop,
    ) -> Result<(), Box<dyn std::any::Any + Send>> {
        use teksilo_platform::external_dnd::{
            ExternalDndEventPayload, ExternalDndHandle, ExternalDragEvent, OutboundOsDragRequest,
        };

        // Deferred blocking outbound (app → OS) drag: run OLE DoDragDrop here,
        // outside the in-app dispatch that started it (Windows). No window is
        // taken out of the manager at this point, so the drag's modal message
        // loop can't strand a borrowed window.
        let payload = match payload.downcast::<OutboundOsDragRequest>() {
            Ok(req) => {
                if let Some(handle) = self
                    .wm
                    .app_context_template()
                    .and_then(|t| t.app_state::<ExternalDndHandle>().cloned())
                {
                    handle.run_pending_outbound_drag(req.window_id);
                }
                return Ok(());
            }
            Err(other) => other,
        };

        let payload = *payload.downcast::<ExternalDndEventPayload>()?;

        let Some(winit_id) = self
            .wm
            .teksilo_to_winit_map()
            .get(&payload.window_id_owner)
            .copied()
        else {
            // Window already torn down — drop silently.
            return Ok(());
        };
        let Some(mut current) = self.wm.take_managed(winit_id) else {
            return Ok(());
        };
        let current_id = current.teksilo_id;

        let current_arc = Some(current.platform_window.window_arc());

        {
            let mut ops = crate::window_manager::WindowOpsImpl::new(
                &mut self.wm,
                event_loop,
                current_id,
                current_arc,
            );
            match payload.event {
                ExternalDragEvent::Entered { data, position } => {
                    current.tree.begin_external_drag(position, data, &mut ops);
                }
                ExternalDragEvent::Moved { position } => {
                    current.tree.update_external_drag(position, &mut ops);
                }
                ExternalDragEvent::Left => {
                    current.tree.cancel_external_drag(&mut ops);
                }
                ExternalDragEvent::Cancelled => {
                    current.tree.abort_external_drag(&mut ops);
                }
                ExternalDragEvent::Dropped { data, position } => {
                    current.tree.end_external_drag(position, data, &mut ops);
                }
                ExternalDragEvent::DragEnded { outcome } => {
                    current.tree.handle_os_drag_ended(outcome, &mut ops);
                }
            }
        }

        // Repaint so hover feedback / drop results show promptly.
        current.request_redraw();
        self.wm.reinsert_managed(winit_id, current);
        Ok(())
    }

    /// Tick gestures on every window with a real `WindowOps` sink so
    /// long-press / drag-tick handlers can open windows.
    fn tick_gestures_in_window(
        &mut self,
        window_id: WindowId,
        now: Instant,
        event_loop: &ActiveEventLoop,
    ) {
        let Some(mut current) = self.wm.take_managed(window_id) else {
            return;
        };
        let current_id = current.teksilo_id;

        let current_arc = Some(current.platform_window.window_arc());

        {
            let mut ops = crate::window_manager::WindowOpsImpl::new(
                &mut self.wm,
                event_loop,
                current_id,
                current_arc,
            );
            current.tree.tick_gestures_with_ops(now, &mut ops);
        }

        self.wm.reinsert_managed(window_id, current);
    }

    /// Report a key to an attached screen reader and say whether the
    /// application still gets it. See [`teksilo_platform::key_report`].
    fn report_key(
        &mut self,
        window_id: WindowId,
        key_event: &winit::event::KeyEvent,
        is_synthetic: bool,
    ) -> teksilo_platform::key_report::KeyDisposition {
        use teksilo_platform::key_report::{KeyDisposition, KeyInput, KeyTarget, KeyboardState};
        let Some(managed) = self.wm.get_by_winit_mut(window_id) else {
            return KeyDisposition::Deliver;
        };
        let target = KeyTarget {
            reader_attached: managed.platform_window.accessibility_active(),
            // A secure field declares a password IME purpose while focused
            // (`TextInputField::secure`, so every `PasswordField`).
            secure_field: managed
                .tree
                .ime_context_for_focused()
                .is_some_and(|ime| ime.purpose == teksilo_core::ime::ImePurpose::Password),
        };
        let keyboard = KeyboardState {
            modifiers: managed.current_modifiers,
            caps_lock: managed.caps_lock_active,
        };
        self.key_report.filter(
            &KeyInput::from_winit(key_event, is_synthetic),
            keyboard,
            target,
        )
    }

    /// What a key press says about the keyboard itself, kept whether or not
    /// the key reaches a widget: the OS toggles Caps Lock, and a chord made
    /// while Alt is held is not an Alt tap, even when a screen reader took
    /// the key.
    fn note_physical_key(&mut self, window_id: WindowId, key_event: &winit::event::KeyEvent) {
        if key_event.state != winit::event::ElementState::Pressed {
            return;
        }
        let key = event_translation::translate_key(&key_event.logical_key);
        let Some(managed) = self.wm.get_by_winit_mut(window_id) else {
            return;
        };
        // Track Caps Lock from the discrete key press (winit's
        // `ModifiersState` carries no lock state), toggling on each key-down
        // edge and pushing the result to `WindowState::caps_lock` for the
        // password-field warning.
        if matches!(key, Some(teksilo_core::event::Key::CapsLock)) {
            managed.caps_lock_active = !managed.caps_lock_active;
            managed
                .state
                .set_caps_lock_from_os(managed.caps_lock_active);
        }
        // Bare-Alt-tap detection: every other KeyDown while Alt is held
        // flips the sticky flag, so the falling edge of `alt_down` only
        // counts as a tap when no chord was composed. winit 0.30 reports the
        // modifier keys as `KeyboardInput` too, on Wayland and X11 alike, but
        // `translate_key` has no Teksilo key for Shift, Control, Alt or Super,
        // so they never count: Alt's own press, and Shift added to it, leave
        // the tap intact. Every key that does translate counts, Caps Lock
        // included.
        if key.is_some() {
            managed.state.note_non_alt_keydown_during_alt();
        }
    }

    fn handle_accessibility_actions(
        &mut self,
        window_id: WindowId,
        event: &WindowEvent,
        event_loop: &ActiveEventLoop,
    ) {
        if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
            managed.platform_window.process_accessibility_event(event);
        }
        self.drain_accessibility_actions(window_id, event_loop);
    }

    /// Dispatch the accessibility actions waiting for a window. Run on every
    /// event of the window, on its non-visual ticks, and when an
    /// accessibility handler wakes the loop for it.
    fn drain_accessibility_actions(&mut self, window_id: WindowId, event_loop: &ActiveEventLoop) {
        let mut a11y_events: Vec<WidgetEvent> = Vec::new();
        if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
            let actions = managed.platform_window.drain_accessibility_actions();
            for req in actions {
                // The adapter names nodes by the ids it was handed, which
                // differ from the tree's own for a node that came back after
                // leaving. One it was handed and has since seen replaced
                // names nothing.
                let Some(req) = managed.tree.resolve_adapter_action(req) else {
                    continue;
                };
                // Synthetic NodeIds (TextRun children emitted by the
                // rich text editor) can't be decoded back to a
                // WidgetId by value alone — look them up via the
                // tree's reverse-map. For plain widget NodeIds the
                // infallible converter is fine.
                let target_widget = if teksilo_core::accessibility::is_synthetic(req.target_node) {
                    managed.tree.widget_for_synthetic(req.target_node)
                } else {
                    Some(teksilo_core::accessibility::node_id_to_widget_id(
                        req.target_node,
                    ))
                };
                let evt = WidgetEvent::AccessAction {
                    action: req.action,
                    target: target_widget,
                    target_node: req.target_node,
                    data: req.data,
                };
                a11y_events.push(evt);
            }
        }
        // A screen reader's action is the user's, as a click is.
        if !a11y_events.is_empty() {
            self.wm.note_user_activation();
        }
        for evt in a11y_events {
            self.dispatch_in_window(window_id, evt, event_loop);
        }
    }

    /// A window's wake target posted a wake (see
    /// `window_manager::wake_routes`).
    ///
    /// A state wake re-arms the target's coalescing *before* anything else,
    /// so a wake made from then on posts again rather than merging into this
    /// one, then dispatches the accessibility actions waiting for the window.
    /// If the window draws and a widget it showed in its last frame has a
    /// relayout or a pull requested off the UI thread pending, it asks for
    /// a redraw: that frame takes it in and repaints, at once. Otherwise it
    /// asks the redraw gate for a non-visual tick, rate-limited: what changed
    /// is state nobody may see, and the tick ends in `post_event`, which
    /// redraws the window if something visible changed after all. That keeps
    /// geometry and accessibility current on a window whose redraw the
    /// compositor withholds, which a redraw request does not reach, and
    /// costs a window that draws no frame for content nobody sees.
    ///
    /// A draw wake (macOS, off the main thread) asks for a redraw unless the
    /// window draws nothing, in which case one is owed for when it draws
    /// again; it changes nothing the UI thread owns, so it ends without
    /// `post_event`.
    fn window_wake(&mut self, wake: WindowWake, event_loop: &ActiveEventLoop) {
        use teksilo_canvas::wake::WakeKind;
        if let Some(trace) = &mut self.idle_trace {
            trace.note_posted_wake(wake.kind);
        }
        match wake.kind {
            WakeKind::Draw => {
                let Some(managed) = self.wm.get_by_winit_mut(wake.window) else {
                    return;
                };
                managed.platform_window.take_posted_wake(WakeKind::Draw);
                if !managed.platform_window.defer_redraw_until_shown() {
                    managed.request_redraw();
                }
            }
            _ => {
                if let Some(managed) = self.wm.get_by_winit_mut(wake.window) {
                    managed.platform_window.take_posted_wake(WakeKind::Layout);
                }
                self.drain_accessibility_actions(wake.window, event_loop);
                if let Some(managed) = self.wm.get_by_winit_mut(wake.window) {
                    if !managed.redraw.draws_nothing(Instant::now())
                        && managed.tree.off_thread_needs_frame()
                    {
                        managed.request_redraw();
                    } else {
                        managed.redraw.request_tick();
                    }
                }
                self.post_event(event_loop);
            }
        }
    }

    /// Everything a frame does except draw: idle callbacks, layout (which
    /// runs the clocks and off-thread pre-passes), size-to-content,
    /// accessibility sync and delivery, and the IME reconcile. A shown
    /// window runs it before rendering; a hidden one runs only this, so its
    /// app state, timers and accessibility tree stay current while it costs
    /// no GPU or compositor time. See [`crate::redraw_gate`].
    fn run_nonvisual_frame(
        &mut self,
        current: &mut crate::window_manager::ManagedWindow,
        event_loop: &ActiveEventLoop,
    ) {
        let current_id = current.teksilo_id;
        let current_arc = Some(current.platform_window.window_arc());
        current.redraw.ran_frame(Instant::now());

        let size = current.platform_window.surface_size();
        let sf = current.platform_window.scale_factor() as f32;
        let proposal = SizeProposal::exact(size.0 as f32 / sf, size.1 as f32 / sf);

        let ran_idle = {
            let mut ops = crate::window_manager::WindowOpsImpl::new(
                &mut self.wm,
                event_loop,
                current_id,
                current_arc,
            );
            run_tree_logic_frame(&mut current.tree, self.idle_budget, proposal, &mut ops)
        };
        if ran_idle && let Some(trace) = &mut self.idle_trace {
            trace.note_idle_callbacks_run();
        }

        // Size-to-content: after layout, measure the content's intrinsic height
        // at the fixed width and grow/shrink the OS window to fit. The native-
        // window modal path lays the tree out at the window's *exact* size, so
        // (unlike the in-tree overlay) the content's natural height never
        // reaches the OS window on its own — a `MessageBox` taller than its
        // fixed height clips, dropping the footer buttons below the client edge.
        // Drive the size through the reactive `WindowState::size()` → `SetSize`
        // path (drained in `post_event`); `last_autosize` guards against
        // a measure → resize → re-measure oscillation. A dialog centred over
        // its owner stays centred on each size it grows to until its size
        // settles (`autosize_centre`), so a dialog wider than it asked for
        // does not hang off to one side; a later change keeps its corner.
        if current.size_to_content.sizes_height() {
            let logical = (size.0 as f32 / sf, size.1 as f32 / sf);
            if let Some(target) =
                size_to_content_target(&current.tree, logical, current.autosize_width_floor)
            {
                let now = (logical.0.round() as u32, logical.1.round() as u32);
                let step = autosize_step(
                    now,
                    target,
                    current.last_autosize,
                    current.autosize_asked_from,
                );
                if step.centre_ends {
                    current.autosize_centre = None;
                }
                if let Some(target) = step.ask {
                    current.last_autosize = Some(target);
                    current.autosize_asked_from = Some(now);
                    // A native dialog cannot be made smaller than its
                    // content; its requested size stays the floor, as the
                    // minimum it opened with.
                    if current.autosize_width_floor.is_some() {
                        current.platform_window.window().set_min_inner_size(Some(
                            winit::dpi::LogicalSize::new(target.0, target.1),
                        ));
                    }
                    current.state.size().set(target);
                    if let Some(centre) = current.autosize_centre {
                        current
                            .state
                            .position()
                            .set(crate::window_manager::centred_on(
                                centre,
                                target,
                                crate::window_manager::frame_insets(
                                    current.platform_window.window(),
                                ),
                            ));
                    }
                }
            }
        }

        // Whether an assistive technology is attached to this window's adapter.
        // The adapter's handlers run off the UI thread and can only leave a
        // flag, so this is where the tree learns of it; both handlers request a
        // redraw, so a change never waits for unrelated activity. Attaching
        // proves nothing about screen readers (a magnifier or an automation
        // harness activates the adapter too) — detaching does, and
        // `set_at_client_attached` is where that asymmetry lives.
        let at_attached = current.platform_window.accessibility_active();
        current.tree.set_at_client_attached(at_attached);
        if at_attached {
            // Connect to the registry now rather than on the first key.
            self.key_report.warm_up();
        }

        // Kept unconditional: `sync_accessibility` is not a pure builder —
        // it steps the framework's live-region announcers, fills the
        // automation announcement ring and maintains `at_version` — and all
        // three must run every frame whether or not anything is listening.
        let _ = current.tree.sync_accessibility();
        let walk = current.tree.a11y_walk_generation();
        let delivered_walk = current.a11y_delivered_walk;
        let delivered_at = current.a11y_delivered_at;
        let now = std::time::Instant::now();
        let needs_full = current.platform_window.take_accessibility_needs_full_tree();
        // What this frame did with the tree; `None` when no assistive
        // technology is attached.
        let mut delivery = None;
        {
            // Whatever reaches the adapter goes through the tree's delivery,
            // which hands a node that left the reader's tree and came back an
            // id the adapter has never had (`deliver_accessibility`). Both
            // closures deliver, and only one of them runs.
            let window_focused = current.focused;
            let tree = std::cell::RefCell::new(&mut current.tree);
            current.platform_window.update_accessibility_with(
                || {
                    // Decided in here, not outside: the closure runs only
                    // when an assistive technology is attached, so a window
                    // nobody is reading neither builds nor throttles.
                    let owed = tree.borrow().accessibility_delivery_owed(window_focused);
                    let decided =
                        a11y_delivery(owed, needs_full, walk != delivered_walk, delivered_at, now);
                    delivery = Some(decided);
                    (decided == A11yDelivery::Deliver)
                        .then(|| tree.borrow_mut().deliver_accessibility(window_focused))
                },
                || tree.borrow_mut().deliver_accessibility(window_focused),
            );
        }
        match delivery {
            Some(A11yDelivery::Deliver) => {
                current.a11y_delivered_walk = walk;
                current.a11y_delivered_at = Some(now);
            }
            // Held back by the throttle, and nothing else will wake the loop
            // once the scroll stops — so the last frame's moves would sit
            // undelivered. Ask for one frame at the end of the window.
            Some(A11yDelivery::HoldUntil(deadline)) => current.tree.request_wake_at(deadline),
            // The adapter already holds this walk's content.
            Some(A11yDelivery::Nothing) => current.a11y_delivered_walk = walk,
            // No assistive technology listening: nothing is built, and
            // nothing is owed a frame.
            None => {}
        }

        // Catch-all IME reconcile: covers focus changes from any source
        // (access actions, programmatic focus, rebuild) that didn't go
        // through `dispatch_in_window`. Layout has settled, so the focused
        // node's descriptor is current. Cheap + deduped, safe every frame.
        Self::settle_ime(current);
    }

    fn handle_redraw_requested(&mut self, window_id: WindowId, event_loop: &ActiveEventLoop) {
        // Pre-render: take the window out so we can construct a real
        // WindowOpsImpl and pass it into layout + render. This lets
        // rebuild-triggered handlers (data-driven state changes,
        // delayed-overlay activation, drag-tick) open windows.
        let Some(mut current) = self.wm.take_managed(window_id) else {
            return;
        };
        // Whatever made this frame, the window no longer waits on a request.
        current.redraw.delivered();
        // X11 reports an iconify with no event of its own: ask winit, which
        // is a round trip there, at most every so often while unfocused.
        current.refresh_minimized(Instant::now());
        current.sync_hidden_for_frame();
        let current_id = current.teksilo_id;
        let current_arc = Some(current.platform_window.window_arc());

        if let Some(trace) = &mut self.idle_trace {
            trace.note_redraw_requested();
        }
        self.run_nonvisual_frame(&mut current, event_loop);
        // A hidden window draws nothing past the one frame it owes on its way
        // to hidden. Its state is current; its pixels wait for the reveal.
        if current.redraw.is_hidden() && !current.redraw.take_transition_frame() {
            if let Some(trace) = &mut self.idle_trace {
                trace.note_hidden_redraw();
            }
            self.wm.reinsert_managed(window_id, current);
            return;
        }

        let frame = {
            let mut ops = crate::window_manager::WindowOpsImpl::new(
                &mut self.wm,
                event_loop,
                current_id,
                current_arc.clone(),
            );
            current.tree.render_with_ops(&mut ops)
        };
        let managed = &mut current;

        let frame = self.sync_window_glyph_atlas(managed, frame, event_loop);

        // The wgpu surface is Rgba8UnormSrgb: it expects linear-light color
        // values and applies sRGB encoding on write. Our Color stores sRGB-
        // encoded bytes (as designers specify them), so we must linearize
        // the clear color here the same way we do for vertex colors.
        let clear = teksilo_render::vertex::srgb_to_linear_rgba(
            managed.tree.theme().colors.surface_main.to_array(),
        );
        match managed.platform_window.render_frame(&frame, clear) {
            teksilo_platform::FrameOutcome::Rendered => {
                managed.redraw.rendered();
                if let Some(trace) = &mut self.idle_trace {
                    trace.note_rendered_frame(window_id);
                }
            }
            teksilo_platform::FrameOutcome::Skipped => {
                // A hidden window's skipped frame is not retried: it would
                // block in acquire for nothing. Its reveal asks again. One the
                // platform reports hidden only when asked (a macOS window
                // AppKit never showed) is marked hidden here; any other is
                // retried, at once the first time and then after a growing
                // wait (see `crate::redraw_gate`).
                let now = Instant::now();
                if !managed.note_occlusion_after_skip(now)
                    && managed.redraw.skipped(now) == crate::redraw_gate::SkippedRetry::Now
                {
                    managed.request_redraw();
                }
                self.wm.reinsert_managed(window_id, current);
                return;
            }
            teksilo_platform::FrameOutcome::NeedsReconfigure => {
                if managed.platform_window.reconfigure_surface() {
                    managed.request_redraw();
                } else {
                    event_loop.exit();
                }
                self.wm.reinsert_managed(window_id, current);
                return;
            }
            teksilo_platform::FrameOutcome::DisplayLost => {
                // Every window in the process shares the one connection, so
                // there is nothing left to draw anywhere. Ask the loop to
                // unwind rather than request a frame that would return here.
                event_loop.exit();
                self.wm.reinsert_managed(window_id, current);
                return;
            }
            teksilo_platform::FrameOutcome::Error(e) => {
                eprintln!("teksilo-app: {e}, reconfiguring surface");
                if managed.platform_window.reconfigure_surface() {
                    managed.request_redraw();
                } else {
                    event_loop.exit();
                }
                self.wm.reinsert_managed(window_id, current);
                return;
            }
        }

        // A live per-frame effect (Pulse / Cycle / caret blink / drag
        // auto-scroll) leaves `frame_requested()` armed after this render.
        // We deliberately do NOT `request_redraw()` here: an immediate
        // redraw request makes winit skip the `WaitUntil` sleep and
        // free-run at the display's refresh rate — the exact 300 fps
        // uncapped behaviour we're removing. Instead the fixed 60 Hz
        // deadline published by `WidgetTree::frame_tick_deadline` (folded
        // into `next_timer_deadline`) drives the next frame: at the
        // deadline, `new_events(ResumeTimeReached)` calls
        // `request_redraw_all()`. This mirrors how the shader-quad
        // animation path has always paced itself, so per-frame animations
        // now show in the idle trace as `resume_time_reached` /
        // `request_redraw_all` rather than `frame_request`.

        self.wm.reinsert_managed(window_id, current);
    }

    fn handle_window_event_inner(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let teksilo_id = self.wm.teksilo_id_for_winit(window_id);

        // An attached screen reader hears of the key before anything acts on
        // it, and may take it for itself (Orca's own commands): a key taken
        // there goes no further. First of all, so that the reader's commands
        // work in a window a modal child blocks too.
        if let WindowEvent::KeyboardInput {
            event: key_event,
            is_synthetic,
            ..
        } = &event
            && self.report_key(window_id, key_event, *is_synthetic)
                == teksilo_platform::key_report::KeyDisposition::Drop
        {
            self.note_physical_key(window_id, key_event);
            self.update_control_flow(event_loop);
            return;
        }

        // A click, a key, a contact: what its handlers ask for this turn is
        // the user's request (see `WindowManager::acts_for_user`).
        if crate::input_routing::is_user_activation(&event) {
            self.wm.note_user_activation();
        }

        // A window blocked by a modal child swallows the user's *input* and
        // bounces it to the child, while still hearing everything the OS says
        // about the window itself. Which is which — and which of the swallowed
        // events is worth raising the child for — is
        // `input_loop::blocked_disposition`.
        if let Some(fid) = teksilo_id
            && self.wm.is_blocked(fid)
        {
            use crate::input_loop::BlockedDisposition;
            match crate::input_loop::blocked_disposition(&event) {
                BlockedDisposition::Deliver => {}
                BlockedDisposition::Swallow => {
                    self.update_control_flow(event_loop);
                    return;
                }
                BlockedDisposition::SwallowAndRaise => {
                    self.wm.refocus_modal_child(fid);
                    self.update_control_flow(event_loop);
                    return;
                }
            }
        }

        self.handle_accessibility_actions(window_id, &event, event_loop);

        // The pointer, touch and OS-gesture family goes through the platform
        // backend and the tree's sample doors rather than through an arm of
        // its own. One place, one clock, one set of suppressors — and the arms
        // themselves are `crate::input_routing`, which a test can drive with a
        // hand-written winit event and a bare tree.
        if crate::input_routing::is_pointer_input_event(&event) {
            self.dispatch_input_in_window(window_id, &event, event_loop);
            self.post_event(event_loop);
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                if let Some(fid) = teksilo_id {
                    // Guarded close: the OS close button / Alt+F4 / Cmd+W
                    // is an interactive gesture, so it runs through the
                    // window's close guard (if any) on the next
                    // `process_pending` tick and may be vetoed. A window
                    // its dialog blocks refuses it and raises the dialog.
                    self.wm.request_close(fid);
                }
            }
            // The OS destroyed the window: Windows destroys an owned window
            // with its owner. One Teksilo closed itself is already gone from
            // the manager; one still in it is forgotten now, or it would
            // stay counted (keeping a `LastWindowClosed` app alive with
            // nothing on screen) and keep its owner blocked.
            WindowEvent::Destroyed => {
                if let Some(fid) = teksilo_id {
                    self.wm.close_window_answering(fid, event_loop);
                }
            }
            WindowEvent::Resized(new_size) => {
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    managed.platform_window.resize(new_size);
                    // Mirror OS-initiated geometry / placement changes
                    // into WindowState so widgets bound to those signals
                    // re-render. The `*_from_os` setters flip the
                    // re-entrancy guard so observers on the signal do
                    // not push the change back out as a WindowCommand.
                    // Covers OS-initiated maximize (drag-to-top-snap on
                    // Wayland/Windows, green-light zoom on macOS) —
                    // query_window_placement reads the winit state and
                    // the Switcher glyph swap on `TitleBar`'s maximize
                    // button (bound to `WindowState::placement`) stays
                    // in sync.
                    let sf = managed.platform_window.scale_factor();
                    let logical_w = (new_size.width as f64 / sf).round().max(0.0) as u32;
                    let logical_h = (new_size.height as f64 / sf).round().max(0.0) as u32;
                    managed.state.set_size_from_os((logical_w, logical_h));
                    let placement = query_window_placement(managed.platform_window.window());
                    managed.state.set_placement_from_os(placement);
                    // Windows reports a minimise as a resize to zero, and a
                    // restore as a resize back.
                    managed.minimized = placement.is_minimized();
                    managed.sync_hidden();
                    // A resize is when the safe area changes: on the one
                    // desktop platform that has one, it is zero in a window
                    // and non-zero once the window covers the camera housing,
                    // which is a resize into full screen.
                    Self::refresh_safe_area(managed);
                    if let Some(trace) = &mut self.idle_trace {
                        trace.note_redraw_request("resize");
                    }
                    managed.request_redraw();
                }
            }
            WindowEvent::Moved(pos) => {
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    let sf = managed.platform_window.scale_factor();
                    let lx = (pos.x as f64 / sf).round() as i32;
                    let ly = (pos.y as f64 / sf).round() as i32;
                    managed.state.set_position_from_os((lx, ly));
                    // It may have moved to a display with another refresh
                    // rate, at the same scale.
                    managed.platform_window.track_display_refresh();
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let mut teksilo_id = None;
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    managed.translation_state.set_scale_factor(scale_factor);
                    managed.platform_window.set_scale_factor(scale_factor);
                    managed.platform_window.track_display_refresh();
                    managed.tree.set_device_scale_factor(scale_factor as f32);
                    // The safe area is reported in points; moving between
                    // displays can change both the scale and the housing.
                    Self::refresh_safe_area(managed);
                    teksilo_id = Some(managed.teksilo_id);
                }
                // Keep the external-DnD backend's idea of the scale current:
                // dragging the window onto a monitor with a different scale
                // mid-drag would otherwise start reporting drops at the wrong
                // place (X11 only — see `ExternalDndGuard::set_scale_factor`).
                if let Some(teksilo_id) = teksilo_id
                    && let Some(handle) = self
                        .wm
                        .app_context_template()
                        .and_then(|t| {
                            t.app_state::<teksilo_platform::external_dnd::ExternalDndHandle>()
                        })
                        .cloned()
                {
                    handle.set_scale_factor(teksilo_id, scale_factor);
                }
                self.typesetter.set_scale_factor(scale_factor as f32);
            }
            WindowEvent::ModifiersChanged(mods) => {
                // Capture state before the alt_down write so we can
                // detect the falling edge without re-reading after.
                let alt_tap_action = if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    managed.current_modifiers = mods.state();
                    managed
                        .translation_state
                        .set_modifiers(event_translation::translate_modifiers(mods.state()));

                    let new_alt = mods.state().alt_key();
                    let prev_alt = managed.state.alt_down().get();
                    let other_pressed = managed.state.other_key_pressed_during_alt();
                    // Alt-tap tracking: surface the OS Alt-held edge on
                    // the window's `alt_down` signal so `MenuLabel` can
                    // gate mnemonic underlines and `MenuBar` can detect
                    // bare-Alt-tap on the falling edge. winit reports
                    // Alt presses through `ModifiersChanged` (not as a
                    // `Key::Alt` KeyDown, which doesn't exist in our
                    // Key enum), so this is the only correct hook.
                    managed.state.set_alt_from_os(new_alt);
                    // Detect the bare-Alt-tap pattern: true → false
                    // with no non-Alt KeyDowns during the hold.
                    if prev_alt && !new_alt && !other_pressed {
                        managed
                            .state
                            .menubar_dispatcher()
                            .and_then(|d| d.on_alt_tap())
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(action) = alt_tap_action {
                    self.apply_menubar_action(window_id, action, event_loop);
                }
            }
            WindowEvent::KeyboardInput {
                event: key_event,
                is_synthetic,
                ..
            } => {
                self.note_physical_key(window_id, &key_event);
                // A synthetic press becomes nothing, here and for the
                // menubar below: see `key_widget_event`.
                let maybe_evt = self.wm.get_by_winit_mut(window_id).and_then(|managed| {
                    crate::input_routing::key_widget_event(
                        &key_event.logical_key,
                        key_event.text.as_deref(),
                        key_event.state,
                        is_synthetic,
                        event_translation::translate_modifiers(managed.current_modifiers),
                    )
                });
                if let Some(evt) = maybe_evt {
                    // Window-level menubar pre-dispatch (F10 / Alt+letter):
                    // intercepts BEFORE the normal focus-based path so the
                    // event reaches the menubar even when focus is in a
                    // TextInput or some other unrelated widget. Matches
                    // Win32's `WM_SYSKEYDOWN` → `DefWindowProc` route.
                    let intercept = if let WidgetEvent::KeyDown { key, modifiers, .. } = &evt {
                        self.wm
                            .get_by_winit_mut(window_id)
                            .and_then(|m| {
                                m.state.menubar_dispatcher().map(|d| {
                                    d.try_handle(&teksilo_core::window::MenubarKeyEvent {
                                        key: *key,
                                        modifiers: *modifiers,
                                    })
                                })
                            })
                            .flatten()
                    } else {
                        None
                    };
                    if let Some(action) = intercept {
                        self.apply_menubar_action(window_id, action, event_loop);
                    } else {
                        self.dispatch_in_window(window_id, evt, event_loop);
                    }
                }
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    if let Some(trace) = &mut self.idle_trace {
                        trace.note_redraw_request("keyboard");
                    }
                    managed.request_redraw();
                }
            }
            WindowEvent::Ime(ime) => {
                // Dedup consecutive empty preedits at the funnel. Some Linux IME
                // backends (ibus / fcitx via winit) flood empty `Ime::Preedit("")`
                // events while a field is focused. The first is meaningful (it
                // clears any active composition); every consecutive repeat is a
                // no-op that would still translate + dispatch through the tree AND
                // wake a full unconditional layout+render pass here. Skip the
                // repeats entirely — neither dispatch nor redraw. Any non-empty
                // preedit (or a Commit / Enabled / Disabled) resets the flag so
                // the next empty preedit is again treated as meaningful.
                let empty_preedit =
                    matches!(&ime, winit::event::Ime::Preedit(t, _) if t.is_empty());
                let skip = if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    crate::window_manager::ime_should_skip_empty_preedit(
                        &mut managed.last_ime_preedit_empty,
                        empty_preedit,
                    )
                } else {
                    false
                };
                if !skip {
                    let maybe_evt = if self.wm.get_by_winit_mut(window_id).is_some() {
                        event_translation::translate_ime(ime)
                    } else {
                        None
                    };
                    if let Some(evt) = maybe_evt {
                        self.dispatch_in_window(window_id, evt, event_loop);
                    }
                    if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                        if let Some(trace) = &mut self.idle_trace {
                            trace.note_redraw_request("ime");
                        }
                        managed.request_redraw();
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(fid) = teksilo_id {
                    self.wm
                        .note_dialog_sign(fid, crate::window_manager::DialogSign::Redrawn);
                }
                self.handle_redraw_requested(window_id, event_loop);
            }
            WindowEvent::ThemeChanged(winit_theme) => {
                self.handle_theme_changed(winit_theme);
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    managed.request_redraw();
                }
            }
            // Pause all looping animations on the unfocused window so it
            // stops waking the event loop at the animation frame
            // interval. The scheduler rebases start_time on resume so
            // the animation phase is continuous — a half-swept
            // indeterminate bar picks up at exactly the same position,
            // not snapped forward by the elapsed unfocused time.
            //
            // On Linux/Windows (winit 0.30) minimize fires `Focused(false)`
            // — no separate minimize event — so this path covers it.
            WindowEvent::Focused(focused) => {
                if focused {
                    // Only one window holds the OS focus. Any other still
                    // claiming it was born with the flag and never told
                    // otherwise; it loses it now, as if `Focused(false)` had
                    // arrived for it.
                    for stale in self.wm.other_focus_claims(window_id) {
                        self.apply_os_focus(stale, false, event_loop);
                    }
                }
                let newly_focused = self.apply_os_focus(window_id, focused, event_loop);
                // A window blocked by a modal child can take the focus without
                // a press reaching it: a click on its title bar, Alt+Tab, its
                // taskbar button. The child comes back to the front, as for a
                // click in the window.
                if focused && let Some(fid) = self.wm.teksilo_id_for_winit(window_id) {
                    // The user is back: the dialogs the application tree
                    // owes them, and those this window owes, are presented
                    // in this turn.
                    self.wm.note_app_focus_gained();
                    self.wm.note_owed_dialogs_focus(fid);
                    self.wm.note_focus_gained(fid);
                    self.wm
                        .note_dialog_sign(fid, crate::window_manager::DialogSign::Focused);
                }
                // A window regaining focus is the natural, zero-idle-cost moment
                // to re-check the OS accessibility preferences (WCAG / EN 301
                // 549 §11.7): the user may have toggled "increase contrast" /
                // "reduce motion" / text scale in System Settings and switched
                // back. `refresh_accessibility_preferences` applies any change
                // to every window (marking them dirty for repaint).
                if focused {
                    // The same moment backstops the desktop watch: a
                    // configuration file written while no signal reached us
                    // (no bus, or a signal missed) is caught here.
                    if self.wm.recheck_os_theme_files() {
                        self.wm.request_redraw_all();
                    }
                    self.wm.refresh_accessibility_preferences();
                }
                // The global native menu (macOS) follows window focus: make the
                // focused window's installed menu the visible one.
                if let Some(teksilo_id) = newly_focused
                    && let Some(handle) = self.wm.app_context_template().and_then(|t| {
                        t.app_state::<teksilo_platform::native_menu::NativeMenuHandle>()
                            .cloned()
                    })
                {
                    handle.activate_window(teksilo_id);
                }
            }
            // Emitted on macOS (the window's occlusion state) and on X11
            // (`VisibilityNotify` fully obscured), never on Wayland or
            // Windows. Handled for parity with Focused so a window hidden
            // behind another — still focused, on macOS — also parks its
            // animations, and so a hidden window stops drawing.
            WindowEvent::Occluded(occluded) => {
                // A dialog AppKit shows again once the window it blocks is
                // back from the Dock gets the raise it was owed.
                if !occluded && let Some(fid) = teksilo_id {
                    self.wm
                        .note_dialog_sign(fid, crate::window_manager::DialogSign::Shown);
                }
                let active = if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    managed.occluded = occluded;
                    // winit's word replaces what asking the platform found.
                    managed.occlusion_probed_at = None;
                    // Both platforms that send this report a window visible
                    // only when it is mapped and not miniaturised, and a
                    // restore without focus sends neither `Focused(true)` nor
                    // a `Resized` that would clear the flag.
                    if !occluded {
                        managed.minimized = false;
                    }
                    managed.sync_hidden();
                    Some(managed.focused && !managed.occluded)
                } else {
                    None
                };
                // `Occluded`, not `WindowDeactivated`: a widget told its
                // pointer was revoked is told *why*, and "the window went
                // behind something" is not "the user went elsewhere".
                if let Some(active) = active {
                    self.set_window_active_in_window(
                        window_id,
                        active,
                        teksilo_core::pointer::CancelReason::Occluded,
                        event_loop,
                    );
                }
                if let Some(managed) = self.wm.get_by_winit_mut(window_id) {
                    // Drive a redraw on both directions. On reveal
                    // (`!occluded`) the render loop stopped pinging while we
                    // were occluded, so without this nudge the window stays
                    // frozen until the user moves the mouse or hits a key. On
                    // occlusion (`occluded`) the active-state flip must reach a
                    // paint pass so the caret hides / selection desaturates
                    // before the window is hidden behind another.
                    managed.request_redraw();
                }
            }
            WindowEvent::ActivationTokenDone { token, .. } => {
                // A `request_activation_token` we issued resolved — hand the
                // freshly-minted token to whoever asked (child-process spawn or
                // an IPC peer). One request outstanding per window, so key by
                // window id and ignore the serial.
                if let Some(cb) = self.wm.take_activation_token_callback(window_id) {
                    cb(Some(token.into_raw()));
                }
            }
            _ => {}
        }

        self.post_event(event_loop);
    }

    fn handle_theme_changed(&mut self, winit_theme: winit::window::Theme) {
        // Read the mode from the WindowManager (the live owner) so a runtime
        // switch to "follow system" via `EventContext::follow_system_theme`
        // is honoured here too. OS-following results carry the id "system".
        match self.wm.theme_mode() {
            ThemeMode::Manual => {} // ignore OS theme changes
            // Trust winit's per-window signal (authoritative on
            // macOS/Windows where OS-colour querying is unimplemented). Each
            // window keeps its density, and an unchanged state sets nothing.
            ThemeMode::FollowSystem => {
                self.wm
                    .apply_os_source(crate::native_theme::OsThemeSource::follow_system(matches!(
                        winit_theme,
                        winit::window::Theme::Dark
                    )));
            }
            // Native adopts the OS's actual colours on Linux; on macOS/Windows
            // (no OS-colour query) it follows winit's authoritative light/dark
            // hint. The shared helper stamps the "system" id.
            ThemeMode::Native => self
                .wm
                .apply_os_theme(Some(matches!(winit_theme, winit::window::Theme::Dark))),
        }
    }
}

impl ApplicationHandler<AppEvent> for TeksiloAppHandler {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Before the first window, and so before the wgpu instance exists: the
        // OpenGL backend cannot present to a window whose display connection it
        // was never told about, and that backend is all a machine without a
        // Vulkan driver has. Idempotent, so a resume after suspend is a no-op.
        teksilo_platform::install_display_handle(event_loop.owned_display_handle());
        // Before the first window, so that it is born knowing whether anything
        // on screen can be seen. A no-op once watching.
        self.wm.watch_session();
        // The operating system's own requests to quit, from the first turn.
        self.watch_os_quit();
        // Likewise for the desktop's appearance, while the theme follows it.
        self.wm.sync_os_theme_watch();

        if !self.initial_created
            && let Some(config) = self.initial_window.take()
        {
            self.wm.create_window(config, event_loop);
            self.initial_created = true;
        }
        // The application tree's mount actions run now, with an event
        // context and the window operations, so an application root can
        // open the first window of an application that starts with none;
        // what they ask for (a dialog, a theme) is drained by the
        // `post_event`, which nothing else brings to an application with no
        // window.
        self.process_pending_mount_actions(event_loop);
        self.post_event(event_loop);
        // Started: an application that would quit before showing anything
        // is a programming error every run hits on its first turn, not a
        // runtime condition, so it fails loudly rather than exiting with
        // status zero and nothing on screen. Judged here rather than before
        // the loop, so an application root can open the first window.
        if !self.started {
            self.started = true;
            if let Some(why) = self
                .quit_policy
                .refuses_startup_without_window(self.wm.window_count())
            {
                panic!("{why}");
            }
        }
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        // Whether the OS woke us or our own timer did. A pen shim reads the
        // digitizer on a thread of its own and buffers what it sees, so an
        // external wake is the signal that a packet may be in flight: the
        // event that woke us and the packet came from the same source, but the
        // shim has not necessarily dispatched it yet. `about_to_wait` arms one
        // catch-up look for exactly that case — see `Self::pen_deadline`.
        self.woken_externally = !matches!(cause, StartCause::ResumeTimeReached { .. });
        if matches!(cause, StartCause::ResumeTimeReached { .. }) {
            if let Some(trace) = &mut self.idle_trace {
                trace.note_resume_time_reached();
                trace.note_request_redraw_all();
            }
            // Redraw only the windows whose frame deadline is actually due —
            // NOT every window. A blanket redraw here pins non-animating
            // windows at the animation frame rate and, on Windows (one
            // RedrawRequested serviced per loop iteration), starves an inactive
            // window's own pending repaint so it freezes. See
            // `WindowManager::request_redraw_due`.
            self.wm.request_redraw_due(Instant::now());
        }
        self.update_control_flow(event_loop);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        // A payload that came with an activation token: every handler sees
        // what it wraps, and the token waits for the first raise or open.
        let (event, token) = unwrap_activation_token(event);
        if let Some(token) = token {
            self.wm.set_dispatch_activation_token(token);
        }
        // Framework-private, so taken before any app handler or observer
        // sees an event.
        let event = match event {
            AppEvent::External(payload) => match payload.downcast::<WindowWake>() {
                Ok(wake) => {
                    self.window_wake(*wake, event_loop);
                    return;
                }
                Err(payload) if payload.is::<AppTreeWake>() => {
                    // Taken before the tick, so a wake made during it posts
                    // again rather than being lost.
                    self.wm.take_app_tree_wake();
                    self.post_event(event_loop);
                    return;
                }
                Err(payload) if payload.is::<CloseAnswered>() => {
                    // A deferred close was answered; the `about_to_wait`
                    // that follows settles it.
                    return;
                }
                Err(payload) if payload.is::<teksilo_platform::os_quit::TerminationSignal>() => {
                    // No one is there to answer a question: every guard is
                    // asked once and saves, and the application ends.
                    self.quit_now(crate::quit::QuitSource::Signal, event_loop);
                    return;
                }
                Err(payload) if payload.is::<teksilo_platform::os_quit::TerminateRequest>() => {
                    if let Ok(request) =
                        payload.downcast::<teksilo_platform::os_quit::TerminateRequest>()
                    {
                        self.os_terminate_request(*request);
                        self.process_pending(event_loop);
                    }
                    return;
                }
                Err(payload) if payload.is::<teksilo_platform::os_quit::SessionEndNow>() => {
                    // The session ends now (Windows' `WM_ENDSESSION`): the
                    // window procedure that posted this runs the messages
                    // until it is finished, and the process may end as
                    // soon as that procedure returns.
                    self.quit_now(crate::quit::QuitSource::SessionEnd, event_loop);
                    if let Ok(request) =
                        payload.downcast::<teksilo_platform::os_quit::SessionEndNow>()
                    {
                        request.finished();
                    }
                    return;
                }
                Err(payload) if payload.is::<WmStateChanged>() => {
                    // A dialog or its owner was minimised or restored (X11);
                    // the `about_to_wait` that follows acts on it.
                    self.wm.reread_wm_states(Instant::now());
                    return;
                }
                Err(payload) => match payload.downcast::<SessionChanged>() {
                    Ok(_) => {
                        self.wm.apply_session(Instant::now());
                        return;
                    }
                    // The desktop's appearance changed: read it again, and
                    // redraw every window when that set a theme or changed a
                    // preference.
                    Err(payload) => match self.wm.route_os_theme_payload(payload) {
                        Ok(changed) => {
                            if changed {
                                self.wm.request_redraw_all();
                            }
                            return;
                        }
                        Err(payload) => {
                            // A web-view engine asking to run. Answered here, with
                            // no redraw: the frames it commits wake their windows
                            // through their pictures, and the events it posts
                            // arrive as payloads of their own.
                            #[cfg(feature = "web-view")]
                            if payload.is::<teksilo_webview::WebViewPumpPayload>() {
                                self.pump_web_views();
                                return;
                            }
                            AppEvent::External(payload)
                        }
                    },
                },
            },
            event => event,
        };
        if let Some(trace) = &mut self.idle_trace {
            trace.note_app_event();
        }
        if let Some(handler) = &mut self.app_event_handler {
            handler(&event);
        }
        // Composed framework observers (see `AppEventObservers` /
        // `TeksiloAppBuilder::register_app_event_observer`) run in
        // addition to the app's own `on_app_event` handler above — this
        // is what lets `teksilo::install_toast` react to
        // `AppEvent::SettingsWriteFailed` without clobbering (or being
        // clobbered by) an app that also called `on_app_event`.
        if let Some(template) = self.wm.app_context_template()
            && let Some(observers) =
                template.app_state::<crate::app_event_observers::AppEventObservers>()
        {
            (observers.0)(&event);
        }
        match event {
            // Backend-event subscription delivery (architecture §9.4): look
            // up the UI-side callback in the shared app context and invoke
            // it with the downcast event payload. The shared template is
            // the same Rc held by every window's tree, so we don't need to
            // route by window.
            AppEvent::SubscriptionEvent { sub_id, event } => {
                // A context-bearing subscription (`subscribe_event_with_ctx`)
                // needs a fresh `EventContext` minted from its window's tree;
                // a plain one dispatches against the shared template with no
                // context. `try_dispatch_subscription_with_ctx` returns `true`
                // when `sub_id` names a context-bearing subscription (so we
                // skip the plain path — a sub_id lives in exactly one map).
                if !self.try_dispatch_subscription_with_ctx(sub_id, &*event, event_loop)
                    && let Some(template) = self.wm.app_context_template()
                {
                    template.dispatch_subscription_event(sub_id, &*event);
                }
            }
            // Hot-reload of an `.ftl` file registered via
            // `I18nConfig::runtime_override(...)`. Architecture §12.7:
            // the reload must *not* trigger a composite rebuild — only
            // the version signal is bumped, and the existing binding
            // system propagates the change to every `LocalizedString`
            // observer. Direction and active locale are unchanged.
            AppEvent::I18nReload { locale, path } => {
                let parsed: Result<teksilo_i18n::LanguageIdentifier, _> = locale.parse();
                match parsed {
                    Ok(loc) => {
                        let reloaded = teksilo_i18n::thread_local::with_active(|mgr| {
                            mgr.reload_from_path(&loc, &path)
                        });
                        match reloaded {
                            Some(Ok(())) => {}
                            Some(Err(e)) => eprintln!(
                                "teksilo-app: hot-reload failed for {loc} ({}): {e}",
                                path.display()
                            ),
                            None => eprintln!(
                                "teksilo-app: hot-reload event for {loc} but no i18n manager installed"
                            ),
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "teksilo-app: hot-reload event with invalid locale `{locale}`: {e}"
                        )
                    }
                }
            }
            // Live cross-process settings sync: a `teksilo-settings`
            // managed file changed on disk (a peer process's write, or
            // harmlessly this process's own write being noticed by its
            // own watcher). Look the path up in the app's
            // `SettingsRegistry` and let it dispatch to whichever
            // `Reloadable` owns it. This must *not* trigger a composite
            // rebuild — `reload_from_disk` only mutates signals/models
            // in place, and the existing reactive binding system
            // propagates the change to every observer, exactly like
            // `I18nReload` above.
            AppEvent::SettingsReload { path } => {
                if let Some(template) = self.wm.app_context_template()
                    && let Some(registry) =
                        template.app_state::<teksilo_settings::SettingsRegistry>()
                    && let Err(e) = registry.dispatch(&path)
                {
                    eprintln!(
                        "teksilo-app: settings reload failed for {}: {e}",
                        path.display()
                    );
                }
            }
            // F3: a `teksilo-settings` `DebouncedWriter` permanently gave
            // up on a queued write (retry cap reached, or a still-failing
            // write forced by process teardown) — the patches for `path`
            // were discarded. `teksilo-app` itself stays widget-agnostic
            // (it cannot depend on `teksilo-widgets`' `Toast` /
            // `NotificationArchive`), so this log is only half the
            // story: the composed `AppEventObservers` dispatched just
            // above also sees this event, and `teksilo::install_toast`
            // (the umbrella crate, which sees both `AppEvent` and
            // `Toast`) registers an observer that turns it into a
            // persistent error toast — see
            // `ToastRegistry::show_settings_write_failed`. This log
            // stays too: a headless/CI app with no toast host installed
            // still needs *some* signal that a write was lost.
            AppEvent::SettingsWriteFailed {
                path,
                attempts,
                dropped_patches,
                message,
            } => {
                eprintln!(
                    "teksilo-app: settings write permanently failed for {} after {} attempts ({} patches dropped): {}",
                    path.display(),
                    attempts,
                    dropped_patches,
                    message
                );
            }
            // Title-bar hosts route their `close()` through this variant so
            // the operation hops back onto the main thread before touching
            // `WindowManager` (see `title_bar_host.rs`). File-dialog
            // backends post their results through the same variant. The
            // arm tries each known payload type in turn; unrecognized
            // payloads are ignored — application-authored `send_external`
            // payloads can coexist with framework-internal ones.
            AppEvent::External(payload) => {
                // Try each framework-internal payload type in turn; the first
                // that consumes it wins. Unrecognized payloads fall through to
                // the title-bar / close-request downcast chain.
                let payload = self
                    .try_route_file_dialog_payload(payload, event_loop)
                    .err();
                let payload = match payload {
                    None => None,
                    Some(payload) => self
                        .try_route_external_dnd_payload(payload, event_loop)
                        .err(),
                };
                let payload = match payload {
                    None => None,
                    Some(payload) => self
                        .try_route_async_completion_payload(payload, event_loop)
                        .err(),
                };
                let payload = match payload {
                    None => None,
                    Some(payload) => self
                        .try_route_native_menu_payload(payload, event_loop)
                        .err(),
                };
                let payload = match payload {
                    None => None,
                    Some(payload) => self.try_route_web_view_payload(payload, event_loop).err(),
                };
                #[cfg(all(feature = "automation", debug_assertions))]
                let payload = match payload {
                    None => None,
                    Some(payload) => self.try_route_automation_payload(payload, event_loop).err(),
                };
                if let Some(payload) = payload {
                    // Did one of the framework's own built-in arms below claim
                    // it? Only what is left over is offered to the app's
                    // `on_external_with_ctx` router (see
                    // `route_external_with_ctx`). The framework arms run FIRST,
                    // so an app router that returns `true` too eagerly can never
                    // swallow a `CloseWindowRequest` or a title-bar synthetic
                    // event; and because the answer is the chain's own trailing
                    // `else`, a built-in arm added later is withheld from the
                    // app router automatically — there is no second list of
                    // "framework-owned types" to keep in step.
                    let mut consumed = true;
                    {
                        if let Some(req) = payload.downcast_ref::<CloseWindowRequest>() {
                            // Custom-chrome (Teksilo-drawn) title-bar close
                            // button — an interactive gesture, so it runs
                            // through the window's close guard (guarded
                            // close), matching the OS close button.
                            self.wm.request_close(req.teksilo_id);
                        } else if let Some(evt) = payload.downcast_ref::<TitleBarSyntheticEvent>() {
                            // Windows custom-chrome wndproc sends this when
                            // `WM_NCLBUTTONUP` fires over a control-button
                            // hit-region. The button's pixels are owned by
                            // the OS so the widget tree never saw the click;
                            // re-issue it as a synthetic tap on the
                            // matching `ControlButton`.
                            self.wm
                                .route_title_bar_synthetic_tap(evt.teksilo_id, evt.target);
                        } else if let Some(evt) = payload.downcast_ref::<TitleBarHoverEvent>() {
                            // Same idea for hover: `WM_NCMOUSEMOVE` over a
                            // control-button hit-region delivers an
                            // entered/leave event the widget tree never
                            // sees, so we drive the matching button's
                            // hover signal explicitly.
                            self.wm.route_title_bar_synthetic_hover(
                                evt.teksilo_id,
                                evt.target,
                                evt.entered,
                            );
                        } else if let Some(inject) = payload.downcast_ref::<SyntheticImeInject>() {
                            // Test / demo hook: replay a scripted IME
                            // sequence into the most recent window's focused
                            // widget through the real dispatch path — no OS
                            // IME needed. Mirrors exactly what the
                            // `WindowEvent::Ime` arm produces.
                            let target = self
                                .wm
                                .most_recent_window()
                                .and_then(|id| self.wm.winit_id_for_teksilo(id));
                            if let Some(winit_id) = target {
                                for evt in inject.events.clone() {
                                    self.dispatch_in_window(winit_id, evt, event_loop);
                                }
                            }
                        } else if let Some(window_id) = repaint_request_window(&*payload) {
                            // The deprecated off-thread "repaint this window":
                            // mark the window's tree paint-dirty; the
                            // unconditional `request_redraw_all()` below then
                            // re-runs the changed widget's `paint()`. Routing
                            // is unchanged until it is removed.
                            let winit_id = self.wm.teksilo_to_winit_map().get(&window_id).copied();
                            if let Some(winit_id) = winit_id
                                && let Some(managed) = self.wm.get_by_winit_mut(winit_id)
                            {
                                managed.tree.mark_all_needs_paint_only();
                            }
                        } else {
                            consumed = false;
                        }
                    }
                    if !consumed {
                        self.route_external_with_ctx(&*payload, event_loop);
                    }
                }
            }
            _ => {}
        }
        // No `on_external_with_ctx` ran to end it (none installed, or a
        // framework arm took the payload): the token is left to a
        // `WindowState::focus()` for this turn only, as after a handler.
        if self.wm.holds_dispatch_activation_token() {
            self.wm.end_dispatch_activation();
        }
        if let Some(trace) = &mut self.idle_trace {
            trace.note_request_redraw_all();
        }
        self.wm.request_redraw_all();
        self.post_event(event_loop);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        self.handle_window_event_inner(event_loop, window_id, event);
    }

    /// winit asks for no redraw after this, and on X11 panics if one is
    /// requested: every window's waker becomes a no-op, whoever still holds
    /// one.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.wm.disconnect_redraw_wakers();
        // On macOS this is the last moment: AppKit ends the process right
        // after its `applicationWillTerminate:`, which reaches here, so
        // `run` never returns to flush, and nothing the application holds
        // is dropped after it.
        self.flush_settings();
        // So the application's state is dropped here, on every platform:
        // each service's `Drop` runs (an analytics adapter's shutdown
        // flush), with the trees and the closures that hold clones of it.
        // What their teardown wrote is flushed after.
        self.app_event_handler = None;
        self.external_ctx_handler = None;
        self.loop_tick = None;
        self.wm.shut_down_app_state();
        self.flush_settings();
        // A signal from now on ends the process, as it would with no loop:
        // only once the settings are on disk, or a signal arriving during
        // the flush would end it with nothing written.
        #[cfg(unix)]
        {
            self.quit_signals = None;
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Drive any registered per-turn closure (the async executor poll when
        // `teksilo-async` is installed) before computing the next control
        // flow. A `true` return means tasks advanced and may have mutated
        // reactive state, so repaint the open windows — mirroring the
        // subscription-delivery redraw in `user_event`.
        if let Some(tick) = &mut self.loop_tick
            && tick()
        {
            self.wm.request_redraw_all();
        }
        // After the loop tick, which may have set state the application
        // tree reads; also how the tree's own deadline, folded into the
        // control flow below, is served when it wakes the loop. What it asks
        // for is drained below.
        self.tick_app_tree(event_loop);
        // The application menu as the application tree declared it, from the
        // first turn on, in the native menu bar too.
        self.wm.sync_application_menu();
        // Once per event-loop turn, which is what the shim's own contract
        // asks for. A digitizer reads on its own thread and buffers; this is
        // the only place that asks it what it saw.
        self.pump_pen_sources(event_loop);
        self.refresh_occluded_band();
        self.process_pending(event_loop);
        // A dialog follows its owner's minimise and restore where the window
        // system does not keep it with its owner.
        self.wm.follow_owners();
        // A raise a dialog is owed is the user's only while the application
        // is: with no window of it focused, it would take the focus from
        // whatever the user went to.
        self.wm.drop_owed_raises_if_inactive();
        // What the tick above, the pen input or a timed gesture left (an
        // intent for the other side, a request) is drained by a
        // `post_event`, which nothing else brings to an application with no
        // window, or to input that arrived outside a window event.
        if self.wm.owes_turn_work() {
            self.post_event(event_loop);
        }
        // Once the turn's handlers have run: a native menu row greys out when
        // nothing on its route takes its intent, read now, so the menu is
        // current when the user opens it.
        self.wm.refresh_native_menu_availability();
        // A redraw becomes withheld with time alone, so the wake targets learn
        // it here, before the ticks that serve a window drawing nothing.
        let now = Instant::now();
        self.wm.recheck_session(now);
        for managed in self.wm.iter_mut() {
            managed.probe_occlusion(now);
        }
        for managed in self.wm.iter() {
            managed.sync_draws_nothing(now);
        }
        self.run_ticks(event_loop);
        self.maybe_exit(event_loop);
        #[cfg(debug_assertions)]
        self.report_stalled_redraws();
        self.update_control_flow(event_loop);
    }
}

/// The window a deprecated `RepaintWindowRequest` payload names, if it is
/// one. A helper because an `else if let` condition cannot carry the
/// `#[allow(deprecated)]` the downcast needs.
#[allow(deprecated)]
fn repaint_request_window(payload: &dyn std::any::Any) -> Option<TeksiloWindowId> {
    payload
        .downcast_ref::<teksilo_core::RepaintWindowRequest>()
        .map(|request| request.window_id)
}

#[cfg(test)]
mod repaint_request_tests {
    use super::repaint_request_window;
    use crate::window_config::TeksiloWindowId;

    /// The deprecated payload is still routed to the window it names, and
    /// nothing else is taken for it.
    #[test]
    #[allow(deprecated)]
    fn a_repaint_window_request_names_its_window() {
        let id = TeksiloWindowId::new(7);
        let request = teksilo_core::RepaintWindowRequest { window_id: id };
        assert_eq!(repaint_request_window(&request), Some(id));
        assert_eq!(repaint_request_window(&"something else"), None);
    }
}

/// Posted by a window's wake target, from any thread, through the routes
/// `window_manager::wake_routes` builds: a state wake (content off the UI
/// thread changed state layout reads, or an accessibility handler left
/// something), or on macOS a draw wake made off the main thread. Private to
/// the framework: taken before any app handler sees an event. See
/// [`TeksiloAppHandler::window_wake`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct WindowWake {
    pub(crate) window: WindowId,
    pub(crate) kind: teksilo_canvas::wake::WakeKind,
}

/// Posted by the session watch, from any thread, each time it hears that
/// whether anything on screen can be seen has changed (on macOS: the
/// displays asleep or awake, the screen locked or unlocked, the session
/// switched out or back). Private to the framework, like [`WindowWake`]; the
/// loop answers with
/// [`WindowManager::apply_session`](crate::window_manager::WindowManager::apply_session).
#[derive(Debug, Clone, Copy)]
pub(crate) struct SessionChanged;

/// Posted by the X11 window-state watch, from its thread, each time the
/// `_NET_WM_STATE` of a modal dialog or of the window it blocks changes (a
/// minimise or a restore, which winit does not report). Private to the
/// framework, like [`WindowWake`]; the loop answers with
/// [`WindowManager::reread_wm_states`](crate::window_manager::WindowManager::reread_wm_states),
/// and the `about_to_wait` that follows lets the dialogs follow their owners.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(
    not(all(unix, not(target_os = "macos"))),
    allow(dead_code, reason = "posted only by the X11 window-state watch")
)]
pub(crate) struct WmStateChanged;

/// An `AppEvent::External` payload with the activation token that came with
/// it: what a single-instance application's listener posts when a second
/// launch forwards its request together with the token its launcher gave it
/// (`XDG_ACTIVATION_TOKEN` on Wayland, `DESKTOP_STARTUP_ID` on X11).
///
/// Every handler sees the inner payload: [`TeksiloAppBuilder::on_app_event`],
/// the observers of [`TeksiloAppBuilder::register_app_event_observer`] and
/// [`TeksiloAppBuilder::on_external_with_ctx`]. The token is held for the
/// first window operation of the `on_external_with_ctx` handler that can use
/// it: `ctx.focus_window` or `ctx.open_window`. One it leaves unused goes, for
/// the rest of the turn only, to a `WindowState::focus()` with no token of its
/// own. An intent the handler sends that its tree leaves to another (the
/// application tree, or a window) takes the unused token with it, so the
/// action that takes it there can open or raise the window with it. Without
/// it a Wayland compositor treats the raise as unsolicited and only asks for
/// attention. Elsewhere the token is ignored.
pub struct WithActivationToken {
    token: String,
    payload: Box<dyn std::any::Any + Send>,
}

impl WithActivationToken {
    /// Wrap `payload` with `token`.
    pub fn new(token: impl Into<String>, payload: impl std::any::Any + Send) -> Self {
        Self {
            token: token.into(),
            payload: Box::new(payload),
        }
    }

    /// The activation token.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// The wrapped payload.
    pub fn payload(&self) -> &(dyn std::any::Any + Send) {
        &*self.payload
    }
}

/// `event` with a [`WithActivationToken`] taken apart: the event carrying
/// the payload it wraps, and its token. Any other event comes back as it is,
/// with no token.
fn unwrap_activation_token(event: AppEvent) -> (AppEvent, Option<String>) {
    match event {
        AppEvent::External(payload) => match payload.downcast::<WithActivationToken>() {
            Ok(wrapped) => {
                let WithActivationToken { token, payload } = *wrapped;
                (AppEvent::External(payload), Some(token))
            }
            Err(payload) => (AppEvent::External(payload), None),
        },
        event => (event, None),
    }
}

impl std::fmt::Debug for WithActivationToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WithActivationToken")
            .finish_non_exhaustive()
    }
}

/// Payload used by `TitleBarHostCallbacks::request_close` to route a
/// host-initiated close back to the main event loop. The host's
/// close callback boxes one of these through `AppEventProxy::send_external`;
/// `TeksiloAppHandler::user_event` downcasts the payload and calls
/// `WindowManager::request_close` so the window runs its close guard and
/// tears down on the next tick (matching the `WindowEvent::CloseRequested`
/// path).
#[derive(Debug, Clone, Copy)]
pub struct CloseWindowRequest {
    pub teksilo_id: TeksiloWindowId,
}

/// Test / demo payload that replays a scripted IME sequence into the
/// focused widget of the
/// [most recent window](crate::WindowManager::most_recent_window), through the same dispatch path the
/// real `WindowEvent::Ime` arm uses — so the full preedit pipeline
/// (document mutation, underline, caret-area reporting, AT selection) can
/// be exercised without an OS input method installed.
///
/// Post it via [`AppEventPoster::post_external`](teksilo_core::AppEventPoster)
/// (reachable from a handler with `ctx.poster()`).
#[derive(Debug, Clone)]
pub struct SyntheticImeInject {
    pub events: Vec<teksilo_core::event::WidgetEvent>,
}

// `TitleBarSyntheticEvent` and `TitleBarHoverEvent` live in
// `teksilo_core::window_chrome` so teksilo-platform (which posts them from
// the Windows wndproc subclass) and teksilo-app (which routes them) can
// both name the type without teksilo-platform depending on teksilo-app.
pub use teksilo_core::{TitleBarHoverEvent, TitleBarSyntheticEvent};

/// A thread-safe handle for posting `AppEvent`s to the UI thread.
///
/// Clone and send to background threads. The event loop wakes up
/// and processes the event like any other input.
#[derive(Clone)]
pub struct AppEventProxy {
    inner: winit::event_loop::EventLoopProxy<AppEvent>,
}

impl AppEventProxy {
    /// Post a background completion event.
    pub fn send_background_complete(&self, operation_id: String) {
        let _ = self
            .inner
            .send_event(AppEvent::BackgroundComplete { operation_id });
    }

    /// Post a background progress event.
    pub fn send_background_progress(&self, operation_id: String, percent: f32, message: String) {
        let _ = self.inner.send_event(AppEvent::BackgroundProgress {
            operation_id,
            percent,
            message,
        });
    }

    /// Post an arbitrary external event.
    pub fn send_external(&self, payload: impl std::any::Any + Send + 'static) {
        let _ = self.inner.send_event(AppEvent::External(Box::new(payload)));
    }

    /// Post a pre-boxed external event. Used by callers that already
    /// hold a `Box<dyn Any + Send>` (notably
    /// `TitleBarHostCallbacks::post_external`, which abstracts the
    /// posting mechanism behind a closure that teksilo-core can hold
    /// without depending on winit).
    pub fn send_external_boxed(&self, payload: Box<dyn std::any::Any + Send>) {
        let _ = self.inner.send_event(AppEvent::External(payload));
    }

    /// Post a backend-event delivery for the given subscription id. Called
    /// by the framework's event-source wrapper from the publisher thread.
    pub fn post_subscription_event(
        &self,
        sub_id: SubscriptionId,
        event: Box<dyn std::any::Any + Send>,
    ) {
        let _ = self
            .inner
            .send_event(AppEvent::SubscriptionEvent { sub_id, event });
    }
}

/// `AppEventProxy` implements [`AppEventPoster`] directly so it can be both the
/// `Arc<dyn AppEventPoster>` every widget tree holds AND handed to background
/// integrations (e.g. the `teksilo-async` executor's cross-thread waker, wired
/// via [`TeksiloAppBuilder::on_ready`]). teksilo-core cannot import winit, so
/// this trait implementation lives here.
impl AppEventPoster for AppEventProxy {
    fn post_subscription_event(
        &self,
        sub_id: SubscriptionId,
        event: Box<dyn std::any::Any + Send>,
    ) {
        let _ = self
            .inner
            .send_event(AppEvent::SubscriptionEvent { sub_id, event });
    }

    fn post_external(&self, payload: Box<dyn std::any::Any + Send>) {
        let _ = self.inner.send_event(AppEvent::External(payload));
    }
}

/// Builder for a Teksilo application.
pub struct TeksiloAppBuilder {
    theme: Theme,
    theme_mode: ThemeMode,
    /// The app's own `ThemeMode::Native` theme, set via
    /// [`native_theme`](Self::native_theme) and handed to the window manager
    /// at `run`.
    native_theme: Option<crate::native_theme::NativeThemeFactory>,
    pen_batching: teksilo_platform::PenBatching,
    typesetter: Option<SharedTypesetter>,
    font_registrars: Vec<Box<dyn teksilo_text::FontRegistrar>>,
    app_event_handler: Option<Box<dyn FnMut(&AppEvent)>>,
    external_ctx_handler: Option<ExternalCtxHandler>,
    on_ready: Vec<Box<dyn FnOnce(AppEventProxy)>>,
    initial_window: Option<WindowConfig>,
    /// When the application quits on its own. See [`QuitPolicy`].
    quit_policy: QuitPolicy,
    /// The application tree's root; see [`app_root`](Self::app_root).
    app_root: Option<AppRootBuilder>,
    /// The application's quit guard; see
    /// [`on_quit_requested`](Self::on_quit_requested).
    quit_guard: Option<teksilo_core::CloseGuard>,
    /// Whether a termination signal quits the application; see
    /// [`termination_signals`](Self::termination_signals).
    termination_signals: bool,
    /// Type-erased adapter for the application's backend event source.
    /// Installed via `event_source<S>(source)`.
    event_source: Option<EventSourceAdapter>,
    /// Application-scoped values keyed by `TypeId`.
    /// Installed via `app_state::<T>(value)` and reachable from any
    /// `BuildContext` via `ctx.app_state::<T>()`.
    app_state_registry: HashMap<TypeId, Box<dyn Any>>,
    /// Internationalization configuration. Installed
    /// via `i18n(I18nConfig)`. When present, an `I18nManager` is built at
    /// `build_headless` / `run` time and registered on the thread-local so
    /// `tr!`-expanded code can resolve translations.
    i18n: Option<I18nConfig>,
    /// Tooltip content entries registered via
    /// [`register_tooltips`](Self::register_tooltips). Frozen into a
    /// thread-local registry in `run` / `build_headless` before the
    /// first frame builds.
    tooltip_contents: Vec<teksilo_widgets::tooltip::TooltipContent>,
    /// OS-correct application paths (config / data dirs). Set via
    /// [`application`](Self::application) or [`app_paths`](Self::app_paths).
    /// Required when `settings_bundle` is set.
    app_paths: Option<teksilo_settings::AppPaths>,
    /// Persistence configuration. When present, the bundle is opened
    /// at startup and each enabled service is registered into the
    /// `app_state` registry under its concrete type.
    settings_bundle: Option<teksilo_settings::SettingsBundle>,
    /// Whether `run()` should start a `SettingsWatcher` over the settings
    /// directories so a peer process's write is picked up live. On by
    /// default whenever a settings bundle is configured — this is the
    /// entire point of `SettingsBundle`'s cross-process-safe writes.
    /// Toggle off via [`settings_watch`](Self::settings_watch) for tests
    /// or environments without a usable filesystem watcher.
    settings_watch_enabled: bool,
    /// Telemetry configuration. When present, the bundle is opened
    /// after `settings_bundle` (it depends on `SettingsStore`) and the
    /// resulting `OpenedTelemetry` + `TelemetryContext` are registered
    /// into the `app_state` registry. The `TelemetryContext` is the
    /// hook the dispatch tap in
    /// [`teksilo_core::widget_tree::WidgetTree::dispatch_intent`] uses to
    /// emit `intent.dispatched` events.
    #[cfg(feature = "telemetry")]
    telemetry_bundle: Option<teksilo_telemetry::TelemetryBundle>,
    /// Per-loop-turn closure + poll flag installed via
    /// [`on_loop_tick`](Self::on_loop_tick). Async-agnostic; moved into the
    /// handler at `run`.
    loop_tick: Option<Box<dyn FnMut() -> bool>>,
    loop_tick_poll: Option<std::rc::Rc<std::cell::Cell<bool>>>,
    /// Whether the windows' trees record announcements. Set by the automation
    /// bridge, their one reader in a running application.
    records_announcements: bool,
}

impl TeksiloAppBuilder {
    pub fn new() -> Self {
        Self {
            theme: teksilo_core::presets::intui::light(),
            theme_mode: ThemeMode::Manual,
            native_theme: None,
            pen_batching: teksilo_platform::PenBatching::default(),
            typesetter: None,
            font_registrars: Vec::new(),
            app_event_handler: None,
            external_ctx_handler: None,
            on_ready: Vec::new(),
            initial_window: None,
            quit_policy: QuitPolicy::default(),
            app_root: None,
            quit_guard: None,
            termination_signals: true,
            event_source: None,
            app_state_registry: HashMap::new(),
            i18n: None,
            tooltip_contents: Vec::new(),
            app_paths: None,
            settings_bundle: None,
            settings_watch_enabled: true,
            #[cfg(feature = "telemetry")]
            telemetry_bundle: None,
            loop_tick: None,
            loop_tick_poll: None,
            records_announcements: false,
        }
    }

    /// Make the windows' trees record what the platform adapters announce,
    /// for a reader in process. The automation bridge is the one reader a
    /// running application has; see
    /// [`WidgetTree::set_records_announcements`].
    // The bridge exists only in a debug build with the `automation` feature,
    // so any other build has no caller.
    #[cfg_attr(not(all(feature = "automation", debug_assertions)), allow(dead_code))]
    pub(crate) fn record_announcements(mut self) -> Self {
        self.records_announcements = true;
        self
    }

    /// Whether [`record_announcements`](Self::record_announcements) was asked.
    #[cfg(all(test, feature = "automation", debug_assertions))]
    pub(crate) fn records_announcements(&self) -> bool {
        self.records_announcements
    }

    /// Identify the application for OS-correct path resolution. The
    /// `(qualifier, organization, application)` triple follows the
    /// `directories` convention (e.g. `("eu", "FernTech", "Skribisto")`).
    /// Required when [`settings`](Self::settings) is used.
    ///
    /// # Panics
    ///
    /// Panics if the OS does not expose a usable home directory
    /// (typically a sandboxed environment with `HOME` unset). Use
    /// [`app_paths`](Self::app_paths) to supply an explicit path
    /// in that situation.
    pub fn application(mut self, qualifier: &str, organization: &str, application: &str) -> Self {
        let paths = teksilo_settings::AppPaths::new(qualifier, organization, application)
            .unwrap_or_else(|| {
                panic!(
                    "TeksiloAppBuilder::application(\"{qualifier}\", \"{organization}\", \
                     \"{application}\"): could not resolve a usable OS config directory. \
                     This typically happens in sandboxed environments with no HOME set. \
                     Use TeksiloAppBuilder::app_paths(AppPaths::for_testing(...) or \
                     AppPaths::from_dirs(...)) to supply an explicit location.",
                )
            });
        self.app_paths = Some(paths);
        self
    }

    /// Provide an explicit [`AppPaths`](teksilo_settings::AppPaths). Used
    /// for portable-mode apps and tests.
    pub fn app_paths(mut self, paths: teksilo_settings::AppPaths) -> Self {
        self.app_paths = Some(paths);
        self
    }

    /// Read the currently-configured `AppPaths`, if any. Used by
    /// builder-extension traits (e.g. `install_toast` in `teksilo`)
    /// that need to open persistent files at install time before
    /// `run` fires.
    pub fn configured_app_paths(&self) -> Option<&teksilo_settings::AppPaths> {
        self.app_paths.as_ref()
    }

    /// The native-theme factory installed with
    /// [`native_theme`](Self::native_theme), if any. Builder-extension traits
    /// and tests read it to see what an install hook put there.
    pub fn configured_native_theme(&self) -> Option<&crate::NativeThemeFactory> {
        self.native_theme.as_ref()
    }

    /// Configure the persistence bundle. When `run`/`build_headless`
    /// fires, the bundle is opened against the configured `AppPaths`
    /// and every active service is registered in `app_state`, where
    /// it becomes reachable via the
    /// [`SettingsExt`](teksilo_settings::SettingsExt) trait.
    ///
    /// # Panics
    ///
    /// Panics during `run` / `build_headless` if no `AppPaths` was
    /// configured first via [`application`](Self::application) or
    /// [`app_paths`](Self::app_paths).
    pub fn settings(mut self, bundle: teksilo_settings::SettingsBundle) -> Self {
        self.settings_bundle = Some(bundle);
        self
    }

    /// Enable or disable the live cross-process settings-reload watcher
    /// started in [`run`](Self::run) (windowed apps only —
    /// [`build_headless`](Self::build_headless) never starts one, since
    /// there is no event loop to post the reload event through).
    ///
    /// **On by default** whenever [`settings`](Self::settings) is
    /// configured: this is what makes a peer process's write to a
    /// shared settings file (Skribisto's one-process-per-project model
    /// shares `general.toml` / `recents.toml` / `window_state.toml`
    /// across every open project) show up in this process's UI with no
    /// restart and no polling. Pass `false` to opt out — e.g. a
    /// sandboxed test environment with no usable filesystem watcher, or
    /// an app that wants to poll `Reloadable::reload_from_disk` on its
    /// own schedule instead.
    pub fn settings_watch(mut self, enabled: bool) -> Self {
        self.settings_watch_enabled = enabled;
        self
    }

    /// Configure the telemetry stack (`teksilo-telemetry`). Mirrors
    /// [`settings`](Self::settings): the bundle is opened during
    /// `run` / `build_headless` against the configured `AppPaths`
    /// **and** the live `SettingsStore`, and the resulting handles
    /// (`OpenedTelemetry`, `TelemetryContext`, `DynamicReporter`) are
    /// registered into `app_state`. Apps reach them via
    /// [`teksilo_telemetry::TelemetryExt`] (`use teksilo_telemetry::TelemetryExt;`).
    ///
    /// # Panics
    ///
    /// Panics during `run` / `build_headless` if no `AppPaths` was
    /// configured first via [`application`](Self::application) or
    /// [`app_paths`](Self::app_paths), or if no
    /// [`settings`](Self::settings) bundle was registered (the
    /// telemetry consent file is opened via the same `AppPaths` and
    /// the endpoint-override key is read from the `SettingsStore`).
    #[cfg(feature = "telemetry")]
    pub fn telemetry(mut self, bundle: teksilo_telemetry::TelemetryBundle) -> Self {
        self.telemetry_bundle = Some(bundle);
        self
    }

    /// Register the application's tooltip string catalog.
    ///
    /// Each [`TooltipContent`](teksilo_widgets::tooltip::TooltipContent)
    /// in the list maps a short stable key (referenced from inline
    /// markup as `[label](:key)`) to a translatable body, an optional
    /// long-form "more" body revealed by the Accordion disclosure
    /// inside a sticky rich tooltip, and an optional keyboard shortcut
    /// (a literal label, or a registered shortcut id via
    /// [`TooltipContent::for_shortcut`](teksilo_widgets::tooltip::TooltipContent::for_shortcut),
    /// which tracks user rebinds).
    ///
    /// The list is merged into the application's tooltip catalog. Register
    /// at app boot, before `run()` — the accumulated catalog is frozen into
    /// a read-only registry before the first frame builds, and installing it
    /// twice panics in debug builds.
    ///
    /// ```ignore
    /// use teksilo_widgets::tooltip::TooltipContent;
    ///
    /// TeksiloAppBuilder::new()
    ///     .register_tooltips(vec![
    ///         TooltipContent::new("save-as", tr!(save_as_tooltip))
    ///             .for_shortcut("app.save_as"),
    ///         TooltipContent::new("autosave", tr!(autosave_tooltip))
    ///             .with_more(tr!(autosave_tooltip_more)),
    ///     ])
    ///     // …
    /// ```
    ///
    /// **Multiple calls accumulate**, like [`Self::register_fonts`] and
    /// [`I18nConfig::compile_in`](teksilo_i18n::I18nConfig::compile_in), so an
    /// application can compose its own catalogue with catalogues shipped by
    /// plugins, extensions or sibling crates. Assigning here instead would mean
    /// a contributor registering one tooltip silently deleted every tooltip the
    /// application had — the failure has no error and no warning, it just makes
    /// rich tooltips stop resolving their `[label](:key)` links.
    ///
    /// On a duplicate key the **first** registration wins; see
    /// [`install_tooltip_registry`](teksilo_widgets::tooltip::install_tooltip_registry).
    pub fn register_tooltips(
        mut self,
        contents: Vec<teksilo_widgets::tooltip::TooltipContent>,
    ) -> Self {
        self.tooltip_contents.extend(contents);
        self
    }

    /// Register a backend event source. Widgets can
    /// then call `BuildContext::subscribe_event(origin, callback)` from
    /// inside their `build()` method to receive events on the UI thread.
    ///
    /// Only one source per application is supported. Subsequent calls
    /// replace the previously registered source.
    pub fn event_source<S: EventSource>(mut self, source: S) -> Self {
        self.event_source = Some(EventSourceAdapter::new(source));
        self
    }

    /// Register an application-defined value of type `T` that any widget
    /// can retrieve via `BuildContext::app_state::<T>()`.
    ///
    /// Each type `T` may be registered at most once; a subsequent call
    /// with the same type replaces the previous value. To share multiple
    /// values of the same logical kind, wrap each in a distinct newtype.
    pub fn app_state<T: 'static>(mut self, value: T) -> Self {
        self.app_state_registry
            .insert(TypeId::of::<T>(), Box::new(value));
        self
    }

    /// Register an app-wide [`DefaultPostRoot`](crate::DefaultPostRoot) hook that wraps every
    /// window's root after its `root_builder` runs.
    ///
    /// Unlike `app_state(DefaultPostRoot::new(..))` — which stores a single
    /// type-keyed value and so silently replaces any previously-registered
    /// hook — this **composes**: each registered hook runs in call order,
    /// each wrapping the previous one's result. So an app that installs the
    /// debug inspector AND the toast host (or any other post-root chrome)
    /// gets both wrappers, not just whichever was installed last. The
    /// earlier-registered hook is the innermost wrapper (it sees the raw
    /// user root); the latest is outermost.
    ///
    /// All framework installers that splice window-level chrome
    /// (`install_inspector_in_debug`, `install_toast*`) route through this,
    /// so their order of installation no longer matters for correctness.
    pub fn register_post_root(mut self, hook: crate::DefaultPostRoot) -> Self {
        use crate::DefaultPostRoot;
        let key = TypeId::of::<DefaultPostRoot>();
        let composed = match self.app_state_registry.remove(&key) {
            Some(existing) => {
                let existing = *existing
                    .downcast::<DefaultPostRoot>()
                    .expect("DefaultPostRoot slot held a non-DefaultPostRoot value");
                let prev = existing.0;
                let next = hook.0;
                DefaultPostRoot(std::rc::Rc::new(move |tree, root_id| {
                    let inner = prev(tree, root_id);
                    next(tree, inner)
                }))
            }
            None => hook,
        };
        self.app_state_registry.insert(key, Box::new(composed));
        self
    }

    /// Register a composable observer that runs on every `AppEvent`,
    /// in addition to (never instead of) the single
    /// [`on_app_event`](Self::on_app_event) handler.
    ///
    /// Unlike `on_app_event` — which stores a single `Option<Box<dyn
    /// FnMut(&AppEvent)>>` and so silently replaces any previously
    /// registered handler — this **composes**: each registered observer
    /// runs, in call order, on every `AppEvent` delivered to the UI
    /// thread. So a framework extension that needs to react to
    /// `AppEvent`s (e.g. `teksilo::install_toast` turning
    /// `AppEvent::SettingsWriteFailed` into a toast) can register its
    /// own observer without clobbering the application's own
    /// `on_app_event` handler, or being clobbered by it, regardless of
    /// install order. Mirrors [`register_post_root`](Self::register_post_root)'s
    /// type-keyed `app_state` composition pattern exactly, but for
    /// event observation instead of post-root window chrome.
    ///
    /// See `TeksiloAppHandler::user_event` for the dispatch order: the
    /// `on_app_event` handler runs first, then every composed observer.
    pub fn register_app_event_observer(mut self, observer: impl Fn(&AppEvent) + 'static) -> Self {
        use crate::app_event_observers::AppEventObservers;
        let key = TypeId::of::<AppEventObservers>();
        let observer = AppEventObservers::new(observer);
        let composed = match self.app_state_registry.remove(&key) {
            Some(existing) => {
                let existing = *existing
                    .downcast::<AppEventObservers>()
                    .expect("AppEventObservers slot held a non-AppEventObservers value");
                let prev = existing.0;
                let next = observer.0;
                AppEventObservers(std::rc::Rc::new(move |event: &AppEvent| {
                    prev(event);
                    next(event);
                }))
            }
            None => observer,
        };
        self.app_state_registry.insert(key, Box::new(composed));
        self
    }

    /// Install the rfd-backed native file-dialog service. Registers a
    /// [`FileDialogHandle`](teksilo_platform::file_dialog::FileDialogHandle)
    /// wrapping an
    /// [`RfdAsyncBackend`](teksilo_platform::file_dialog::RfdAsyncBackend)
    /// into the app-state registry. Reachable from any handler via
    /// `ctx.app_state::<FileDialogHandle>()`, or — with
    /// `use teksilo_platform::file_dialog::EventContextFileDialogExt;` —
    /// directly via `ctx.pick_file(req, |result, ctx| ...)`.
    ///
    /// Apps that ship a custom or mock backend bypass this and call
    /// `.app_state(FileDialogHandle::new(my_backend))` directly.
    #[cfg(feature = "rfd-backend")]
    pub fn install_file_dialog(mut self) -> Self {
        use teksilo_platform::file_dialog::{FileDialogHandle, RfdAsyncBackend};
        let handle = FileDialogHandle::new(RfdAsyncBackend::new());
        self.app_state_registry
            .insert(TypeId::of::<FileDialogHandle>(), Box::new(handle));
        self
    }

    /// Install the external (OS) drag-and-drop service. Registers an
    /// [`ExternalDndHandle`](teksilo_platform::external_dnd::ExternalDndHandle)
    /// wrapping the platform's default backend
    /// ([`default_backend`](teksilo_platform::external_dnd::default_backend) —
    /// raw `NSDraggingDestination` on macOS, OLE on Windows, `wl_data_device`
    /// on Wayland, a no-op on X11) into the app-state registry.
    ///
    /// Once installed, every window is registered as an OS drop target on
    /// creation (and detached on close) by the window manager. Drops surface
    /// to widgets through the normal drag handlers (`on_drag_hover` /
    /// `on_drag_leave` / `on_drop`) with `payload.is_external()` true — the
    /// ready-made `DropZone` widget consumes them.
    ///
    /// Apps that ship a custom backend bypass this and call
    /// `.app_state(ExternalDndHandle::new(my_backend))` directly.
    pub fn install_external_dnd(mut self) -> Self {
        use teksilo_platform::external_dnd::{ExternalDndHandle, default_backend};
        let handle = ExternalDndHandle::new(default_backend());
        self.app_state_registry
            .insert(TypeId::of::<ExternalDndHandle>(), Box::new(handle));
        self
    }

    /// Install the native (OS) menu service. Registers a
    /// [`NativeMenuHandle`](teksilo_platform::native_menu::NativeMenuHandle)
    /// wrapping the platform's default backend (a real `NSMenu` on macOS, a
    /// no-op elsewhere) into the app-state registry.
    ///
    /// Once installed, the macOS menu bar shows the application menu the
    /// application tree declares (`ctx.set_application_menu(model)`, the
    /// `ApplicationMenuExt` trait): whenever the key window has no menu of
    /// its own (nor belongs to a window that has one, as a modal dialog
    /// does), and while no window is open; with none declared, a default App
    /// menu whose Quit asks the application to quit. A window's own
    /// [`MenuBar`](teksilo_widgets::MenuBar) built with
    /// `from_model(..).native_on_macos(..)` mirrors its
    /// [`MenuModel`](teksilo_widgets::MenuModel) into the bar instead while
    /// that window is key. Item activations route back through the usual
    /// `Intent`/`Action` pipeline, and the bar follows window focus
    /// automatically (see the `WindowEvent::Focused` arm).
    ///
    /// Apps that ship a custom backend bypass this and call
    /// `.app_state(NativeMenuHandle::new(my_backend))` directly.
    pub fn install_native_menu(mut self) -> Self {
        use teksilo_platform::native_menu::{NativeMenuHandle, default_backend};
        let handle = NativeMenuHandle::new(default_backend());
        self.app_state_registry
            .insert(TypeId::of::<NativeMenuHandle>(), Box::new(handle));
        self
    }

    /// Register an `I18nConfig`. Constructs an
    /// `I18nManager` at startup, installs it on the thread-local, and
    /// seeds the widget tree with the resolved initial locale and layout
    /// direction. Without this call, `tr!`-expanded code falls back to
    /// returning the literal key as a placeholder.
    pub fn i18n(mut self, config: I18nConfig) -> Self {
        self.i18n = Some(config);
        self
    }

    /// Set a fixed theme (implies `ThemeMode::Manual`).
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self.theme_mode = ThemeMode::Manual;
        self
    }

    /// Set how the application resolves its theme.
    ///
    /// - `ThemeMode::Manual` — use the theme set via `.theme()` (default).
    /// - `ThemeMode::FollowSystem` — auto-switch between light/dark built-in themes.
    /// - `ThemeMode::Native` — read colors from OS desktop environment config.
    pub fn theme_mode(mut self, mode: ThemeMode) -> Self {
        self.theme_mode = mode;
        self
    }

    /// Give `ThemeMode::Native` a theme of the app's own.
    ///
    /// Whenever Native mode needs a theme (a window opening, the desktop's
    /// light or dark state changing, a switch to Native mode at runtime), `f`
    /// is asked first with a [`NativeThemeRequest`](crate::NativeThemeRequest)
    /// carrying what the desktop reports, whether the theme is to be dark and
    /// the density wanted. Answer `Some(theme)` to use it, for example a
    /// design-language preset rebuilt over the desktop's colours so Native
    /// mode brings that preset's geometry and chrome; answer `None` to keep
    /// the default, IntUI coloured by the desktop
    /// ([`presets::intui::from_os`](teksilo_core::presets::intui::from_os)).
    ///
    /// The theme the app answers with is given the id `"system"`, like every
    /// theme that follows the OS, so a `ThemeSwitcher` shows it as "follow
    /// OS", and is projected onto the density it was asked at with
    /// [`Theme::with_density`], which is the density of the window it is
    /// for. Setting a factory does not select Native mode; combine it with
    /// [`theme_mode`](Self::theme_mode). See
    /// [`resolve_native_theme`](crate::resolve_native_theme).
    pub fn native_theme(
        mut self,
        f: impl Fn(&crate::NativeThemeRequest<'_>) -> Option<Theme> + 'static,
    ) -> Self {
        self.native_theme = Some(Box::new(f));
        self
    }

    /// Choose how a drained pen batch reaches the tree.
    ///
    /// A digitizer outruns the window's message rate, so one poll routinely
    /// drains several packets.
    /// [`PerPacket`](teksilo_platform::PenBatching::PerPacket) — the default —
    /// spends a whole tree dispatch on each;
    /// [`Coalesce`](teksilo_platform::PenBatching::Coalesce) spends one per
    /// drain and hands the intermediate positions to handlers through
    /// [`EventContext::coalesced`](teksilo_core::EventContext::coalesced),
    /// each keeping its own time and axes.
    ///
    /// A drawing surface wants `Coalesce` and must read `coalesced` — see
    /// `docs/ink.md`. Anything that only watches moves should stay on the
    /// default, because `Coalesce` genuinely produces fewer `PointerMove`s.
    ///
    /// Applies to every window; set it before `run`.
    pub fn pen_batching(mut self, mode: teksilo_platform::PenBatching) -> Self {
        self.pen_batching = mode;
        self
    }

    pub fn typesetter(mut self, typesetter: SharedTypesetter) -> Self {
        self.typesetter = Some(typesetter);
        self
    }

    /// Register additional fonts (e.g. a theme's font family) into the
    /// shared typesetter at startup, *before* any text is shaped — so a
    /// theme that sets `typography.body.family = "Roboto"` resolves
    /// correctly instead of silently falling back to the bundled Inter.
    ///
    /// A theme preset typically exposes a `FontRegistrar` the app passes
    /// here:
    /// ```ignore
    /// TeksiloAppBuilder::new()
    ///     .theme(material3::light())
    ///     .register_fonts(material3::font_registrar())
    ///     .run();
    /// ```
    pub fn register_fonts(mut self, registrar: impl teksilo_text::FontRegistrar + 'static) -> Self {
        self.font_registrars.push(Box::new(registrar));
        self
    }

    /// Register a handler for `AppEvent`s received from background threads.
    pub fn on_app_event(mut self, handler: impl FnMut(&AppEvent) + 'static) -> Self {
        self.app_event_handler = Some(Box::new(handler));
        self
    }

    /// Register a router for [`AppEvent::External`] payloads that needs to
    /// **open, find or focus windows** — see [`ExternalCtxHandler`].
    ///
    /// [`on_app_event`](Self::on_app_event) is the hook for reacting to an event;
    /// this is the hook for *acting on the window set* because of one. The
    /// difference is not stylistic: `on_app_event` receives `&AppEvent` and
    /// nothing else, and `EventContext::open_window` panics on a standalone
    /// context, so there is no way to open a window from there at all.
    ///
    /// The handler runs against the tree of the
    /// [most recent window](crate::WindowManager::most_recent_window) (the
    /// one opened or focused last), or against the application tree (see
    /// [`app_root`](Self::app_root)) when no window is open, with a real
    /// [`WindowOps`](teksilo_core::WindowOps) sink either way, and is
    /// consulted **only** for payloads that no framework router and no built-in
    /// downcast arm claimed — so it never has to defend against
    /// `CloseWindowRequest` and friends. Return `true` when the payload was
    /// yours.
    ///
    /// Unlike [`register_app_event_observer`](Self::register_app_event_observer),
    /// this is a single slot: calling it twice replaces the first router, the
    /// same way `on_app_event` does.
    ///
    /// ```ignore
    /// // A single-instance app: a second launch forwards its argv over a socket,
    /// // the listener posts it with `AppEventProxy::send_external`, and this
    /// // opens (or raises) the document window — the "document window" recipe in
    /// // docs/multi-window.md, driven from off the UI thread.
    /// .on_external_with_ctx(move |payload, ctx| {
    ///     let Some(req) = payload.downcast_ref::<OpenDocument>() else {
    ///         return false;
    ///     };
    ///     let wid = window_id_for(&req.path);
    ///     match ctx.find_window(&wid) {
    ///         Some(id) => ctx.focus_window(id),
    ///         None => { ctx.open_window(document_window_config(&req.path)); }
    ///     }
    ///     true
    /// })
    /// ```
    pub fn on_external_with_ctx(
        mut self,
        handler: impl FnMut(
            &(dyn std::any::Any + Send),
            &mut teksilo_core::widget::EventContext,
        ) -> bool
        + 'static,
    ) -> Self {
        self.external_ctx_handler = Some(Box::new(handler));
        self
    }

    /// Register a callback that receives an `AppEventProxy` once the event loop is ready.
    /// Use this to hand the proxy to background threads that need to post commands.
    /// May be called more than once; all registered callbacks fire in order
    /// (e.g. `install_async` registers one to wire the executor's waker).
    pub fn on_ready(mut self, handler: impl FnOnce(AppEventProxy) + 'static) -> Self {
        self.on_ready.push(Box::new(handler));
        self
    }

    /// Register a closure run once per event-loop turn (at the top of
    /// `about_to_wait`) plus a shared poll flag. Returning `true` from the
    /// closure means it advanced work that may have mutated UI state, which
    /// triggers a repaint of all windows. While `poll_source` is set the loop
    /// stays in [`ControlFlow::Poll`] so the closure keeps running; when it
    /// clears, the loop sleeps until the next event (off-thread wakes arrive
    /// via [`AppEventProxy`]).
    ///
    /// General-purpose and async-agnostic — `teksilo-app` only ever sees
    /// `FnMut`. The optional `teksilo-async` crate uses this to drive a
    /// main-thread executor; nothing in the core loop depends on a runtime.
    pub fn on_loop_tick(
        mut self,
        poll_source: std::rc::Rc<std::cell::Cell<bool>>,
        tick: impl FnMut() -> bool + 'static,
    ) -> Self {
        self.loop_tick = Some(Box::new(tick));
        self.loop_tick_poll = Some(poll_source);
        self
    }

    /// Configure the window the application opens at startup: build a
    /// [`WindowConfig`] and pass it here.
    ///
    /// Optional. Under the default [`QuitPolicy::LastWindowClosed`] an
    /// application without one must open its first window from the
    /// application tree's
    /// [`run_after_mount`](teksilo_core::BuildContext::run_after_mount)
    /// (see [`app_root`](Self::app_root)): with no window open once startup
    /// has run, [`run`](Self::run) panics on its first turn, since the
    /// application would quit before showing anything. Under
    /// [`QuitPolicy::Explicit`] it may run with no window at all.
    ///
    /// ```ignore
    /// TeksiloAppBuilder::new()
    ///     .theme(teksilo_core::presets::intui::light())
    ///     .initial_window(
    ///         WindowConfig::new()
    ///             .title("My App")
    ///             .size(800, 600)
    ///             .root(|tree, _state| tree.add(MyRoot::new())),
    ///     )
    ///     .run();
    /// ```
    pub fn initial_window(mut self, config: WindowConfig) -> Self {
        self.initial_window = Some(config);
        self
    }

    /// Set when the application quits on its own. The default,
    /// [`QuitPolicy::LastWindowClosed`], quits when the last window closes.
    pub fn quit_policy(mut self, policy: QuitPolicy) -> Self {
        self.quit_policy = policy;
        self
    }

    /// Build the root of the **application tree**, the widget tree that
    /// belongs to no window and hosts application code that must run with
    /// none open: actions an application-level menu fires, a
    /// single-instance handler, the quit. `root` adds the root widget to
    /// the tree and returns its id, as a window's root builder does.
    ///
    /// The tree is never painted. Its handlers run with
    /// [`EventContext::window`](teksilo_core::widget::EventContext::window)
    /// `None`; everything that names a window by id works there, including
    /// opening one, and nothing about "this window" does (no parent handle
    /// for a file dialog, no `close_window`). It shares the application's
    /// theme, locale, text scale and app state with the windows.
    ///
    /// Register application-wide actions in the root's `build()` with
    /// [`BuildContext::register_action_global`](teksilo_core::BuildContext::register_action_global),
    /// and run code once the tree is mounted with
    /// [`BuildContext::run_after_mount`](teksilo_core::BuildContext::run_after_mount):
    /// that runs at startup, after the initial window if there is one, and
    /// can open the first window of an application started with no initial
    /// window, under either [`QuitPolicy`].
    ///
    /// What needs a window is not drawn there. A modal request
    /// ([`EventContext::present_modal`](teksilo_core::widget::EventContext::present_modal),
    /// a `MessageBox`) is presented in the window used last of those not
    /// minimised (else the one used last), or in a window of its own when
    /// none is open. Asked in answer to the user's input (a click, a key, a
    /// menu row, a screen reader's action, or a payload carrying an
    /// activation token), that window is restored and brought to the front;
    /// otherwise (a timer, another thread) it asks for the user's attention
    /// and stays where it is, so the application takes the focus from no
    /// other. The request must build its content with
    /// `ModalRequest::deferred`; one presenting an existing widget is
    /// refused. A file dialog has no parent window and reports an error. A
    /// toast is shown in every window. An overlay or an animated widget is
    /// never shown, and a debug build says so; a tooltip or a `WebView` is
    /// never shown either.
    ///
    /// Without this, the application tree has an empty root of its own.
    pub fn app_root(
        mut self,
        root: impl FnOnce(&mut WidgetTree) -> teksilo_core::WidgetId + 'static,
    ) -> Self {
        self.app_root = Some(Box::new(root));
        self
    }

    /// Install the application's **quit guard**, asked when a guarded quit
    /// ([`EventContext::quit`](teksilo_core::widget::EventContext::quit),
    /// the App menu's Quit) begins, before any window's close guard and
    /// before any window closes, as a window's
    /// [`on_close_requested`](teksilo_core::WindowConfig::on_close_requested)
    /// guard is asked before the window closes. It runs in the application
    /// tree (see [`app_root`](Self::app_root)), so it can present a dialog
    /// with no window open, and answers with the same
    /// [`CloseResponse`](teksilo_core::CloseResponse): `Close` lets the quit
    /// go on to the windows, `Veto` abandons it, and `Defer` holds it until
    /// its [`CloseDecision`](teksilo_core::CloseDecision) is answered. A
    /// forced quit
    /// ([`quit_forced`](teksilo_core::widget::EventContext::quit_forced))
    /// does not ask it. A quit the operating system leaves no time to ask
    /// for (a session ending now, a termination signal) asks it once, so
    /// the application can save, and goes on whatever it answers.
    ///
    /// The application is asked first, as Qt's `QEvent::Quit` reaches the
    /// application before its windows are closed and Electron's
    /// `before-quit` precedes the windows' `close` events: the windows close
    /// as the quit goes, so a refusal asked last would come with them gone.
    ///
    /// ```ignore
    /// .on_quit_requested(move |ctx| {
    ///     if !downloads.is_busy() {
    ///         return CloseResponse::Close;
    ///     }
    ///     let decision = CloseDecision::new();
    ///     let answer = decision.clone();
    ///     ctx.present_message_box(
    ///         MessageBox::question(lit!("Downloads are running. Quit anyway?"))
    ///             .buttons(MessageBoxButtons::YesNo)
    ///             .on_result(move |r, _ctx| match r.button {
    ///                 StandardButton::Yes => answer.proceed(),
    ///                 _ => answer.cancel(),
    ///             }),
    ///     );
    ///     CloseResponse::Defer(decision)
    /// })
    /// ```
    ///
    /// The guard reads
    /// [`ctx.quit_source()`](teksilo_core::widget::EventContext::quit_source)
    /// to tell a user's quit from a session ending, when it can save
    /// without asking.
    ///
    /// Not consulted by [`build_headless`](Self::build_headless), which has
    /// no quit.
    pub fn on_quit_requested(
        mut self,
        guard: impl Fn(&mut teksilo_core::widget::EventContext) -> teksilo_core::CloseResponse + 'static,
    ) -> Self {
        self.quit_guard = Some(std::rc::Rc::new(guard));
        self
    }

    /// Whether `SIGTERM` and `SIGHUP` quit the application through its
    /// guards (Unix; ignored elsewhere). On by default: while the event
    /// loop runs, either signal asks the application's guard and every
    /// window's once, with
    /// [`QuitSource::Signal`](teksilo_core::QuitSource::Signal), so a
    /// document's guard can save without asking; nothing can refuse, the
    /// settings are written, and the application ends. Without it they end
    /// the process at once, as they do for any program, with nothing
    /// saved. `SIGINT` (Ctrl+C in the terminal that started the
    /// application) keeps its default either way.
    ///
    /// Turn it off for an application that handles these signals itself
    /// (with its own `signal-hook` registration, for instance); its
    /// handlers then run alone. Before the loop starts and once it has
    /// ended, the two signals keep their default meaning. One the process
    /// was started with ignored (`SIGHUP` under `nohup`) is left ignored
    /// throughout.
    #[doc(hidden)]
    pub fn termination_signals(mut self, handled: bool) -> Self {
        self.termination_signals = handled;
        self
    }

    /// Open the configured settings bundle (if any) and register
    /// each service in the app-state registry.
    fn install_settings(&mut self) -> Option<teksilo_settings::OpenedSettings> {
        let bundle = self.settings_bundle.take()?;
        let paths = self.app_paths.clone().expect(
            "TeksiloAppBuilder::settings(...) requires .application(...) or .app_paths(...) \
             to be set first so persistence has a target directory.",
        );
        match bundle.open(&paths) {
            Ok(opened) => {
                self.app_state_registry.insert(
                    TypeId::of::<teksilo_settings::SettingsStore>(),
                    Box::new(opened.store.clone()),
                );
                if let Some(w) = &opened.window_state {
                    self.app_state_registry.insert(
                        TypeId::of::<teksilo_settings::WindowStateService>(),
                        Box::new(w.clone()),
                    );
                }
                // Reachable from any handler via
                // `ctx.app_state::<teksilo_settings::SettingsRegistry>()`,
                // so application code can register its own ad hoc
                // `SettingsFile` / `PersistedListModel` / `MruList`
                // handles into the very same registry a `SettingsWatcher`
                // event gets dispatched through — not just the two
                // services the bundle itself opens.
                self.app_state_registry.insert(
                    TypeId::of::<teksilo_settings::SettingsRegistry>(),
                    Box::new(opened.registry.clone()),
                );
                Some(opened)
            }
            Err(e) => {
                eprintln!("teksilo-app: failed to open settings bundle: {e}");
                None
            }
        }
    }

    /// Open the configured telemetry bundle (if any) and register the
    /// resulting handles into `app_state` so the dispatch tap and the
    /// `TelemetryExt` accessors can reach them. Must be called *after*
    /// `install_settings`, because `TelemetryBundle::open` reads the
    /// endpoint-override key from the live `SettingsStore`.
    ///
    /// # Panics
    ///
    /// Panics if `.telemetry(...)` was called without prior
    /// `.application(...)` / `.app_paths(...)`, or without a
    /// `.settings(...)` bundle. Both are hard requirements: the
    /// consent file needs an `AppPaths` target, and the runtime
    /// endpoint-override key lives in the `SettingsStore`.
    /// Fail-closed by design — a misconfigured app must not silently
    /// skip telemetry installation.
    #[cfg(feature = "telemetry")]
    fn install_telemetry(&mut self, settings: Option<&teksilo_settings::SettingsStore>) {
        let Some(bundle) = self.telemetry_bundle.take() else {
            return;
        };
        let paths = self.app_paths.clone().expect(
            "TeksiloAppBuilder::telemetry(...) requires .application(...) or .app_paths(...) \
             to be set first so the consent file has a target directory.",
        );
        let store = settings.expect(
            "TeksiloAppBuilder::telemetry(...) requires .settings(...) so the runtime \
             endpoint-override key can be read from the SettingsStore. \
             Add .settings(SettingsBundle::new()) before .telemetry(...).",
        );
        match bundle.open(&paths, store) {
            Ok(opened) => {
                // Register the OpenedTelemetry under its concrete type
                // so widgets can access it via TelemetryExt::telemetry().
                self.app_state_registry.insert(
                    TypeId::of::<teksilo_telemetry::OpenedTelemetry>(),
                    Box::new(opened.clone()),
                );
                // Register the dispatch hook under the teksilo-core type.
                // The dispatch tap looks this up by TypeId.
                let session_id = generate_session_id();
                let tcx = teksilo_core::telemetry::TelemetryContext {
                    reporter: opened.reporter.clone()
                        as std::rc::Rc<dyn teksilo_core::telemetry::UsageReporter>,
                    session_id,
                    schema_version: opened.event_schema_version,
                };
                self.app_state_registry.insert(
                    TypeId::of::<teksilo_core::telemetry::TelemetryContext>(),
                    Box::new(tcx),
                );
            }
            Err(e) => {
                eprintln!("teksilo-app: failed to open telemetry bundle: {e}");
            }
        }
    }

    /// Build a headless app for testing (no window, no GPU).
    pub fn build_headless(mut self) -> HeadlessApp {
        // Install the tooltip registry before anything else — widgets
        // that read from it during their first build (e.g. rich
        // tooltips looking up their :key) need it available.
        if !self.tooltip_contents.is_empty() {
            teksilo_widgets::tooltip::install_tooltip_registry(std::mem::take(
                &mut self.tooltip_contents,
            ));
        }

        // Open settings (if a bundle was configured) and register the
        // services into `app_state_registry` so they're reachable from
        // any handler via the SettingsExt trait.
        let opened_settings = self.install_settings();

        // Open telemetry (if a bundle was configured). Must come after
        // install_settings — TelemetryBundle reads the endpoint-override
        // key from the SettingsStore.
        #[cfg(feature = "telemetry")]
        self.install_telemetry(opened_settings.as_ref().map(|s| &s.store));

        let mut tree = WidgetTree::new().with_theme(self.theme.clone());
        let mut app_tree = self
            .app_root
            .is_some()
            .then(|| WidgetTree::new().with_theme(self.theme.clone()));

        let typesetter = self
            .typesetter
            .take()
            .unwrap_or_else(SharedTypesetter::new_with_default_font);
        // Install app/theme fonts before any text is shaped, so a theme's
        // `typography.*.family` resolves instead of falling back to the
        // bundled default.
        for registrar in &self.font_registrars {
            typesetter.apply_font_registrar(registrar.as_ref());
        }
        tree = tree.with_text_backend(typesetter.as_text_backend());
        app_tree =
            app_tree.map(|app_tree| app_tree.with_text_backend(typesetter.as_text_backend()));
        // Auto-register so rich-text widgets can reach the shared typesetter
        // via `ctx.app_state::<SharedTypesetter>()` in headless tests too.
        self.app_state_registry.insert(
            std::any::TypeId::of::<SharedTypesetter>(),
            Box::new(typesetter),
        );
        // Every application has an application menu's holder, empty until
        // the application tree declares one.
        self.app_state_registry
            .entry(std::any::TypeId::of::<teksilo_widgets::menu::ApplicationMenu>())
            .or_insert_with(|| Box::new(teksilo_widgets::menu::ApplicationMenu::new()));

        // Install the i18n manager (if any) and seed the tree with the
        // resolved initial locale and layout direction. Must happen before
        // the root builder runs so that any `tr!` calls inside `build()`
        // resolve against the correct locale on first build.
        let i18n_manager = self.i18n.as_ref().map(|cfg| install_i18n(&mut tree, cfg));
        teksilo_widgets::localize_standard_shortcuts(tree.shortcut_registry_mut());
        if let (Some(app_tree), Some(mgr)) = (app_tree.as_mut(), i18n_manager.as_ref()) {
            app_tree.set_locale(mgr.locale_signal().get().to_string());
            app_tree.set_layout_direction(mgr.direction_signal().get());
        }

        // Install the app-state registry (if any) before running the root
        // builder so that widgets' `build()` methods can call
        // `ctx.app_state::<T>()`.
        if !self.app_state_registry.is_empty() {
            let ctx =
                std::rc::Rc::new(TreeAppContext::empty().with_app_state(self.app_state_registry));
            if let Some(app_tree) = app_tree.as_mut() {
                app_tree.set_app_context(ctx.clone());
            }
            tree.set_app_context(ctx);
        }

        // The application tree, as `run` builds it, minus the window
        // manager: no window, never painted. Both trees keep what they leave
        // unclaimed, for `HeadlessApp::route_unclaimed_intents`.
        let application_scope = teksilo_core::ApplicationScope::new();
        if let (Some(app_tree), Some(root)) = (app_tree.as_mut(), self.app_root.take()) {
            app_tree.set_records_unclaimed_intents(true);
            tree.set_records_unclaimed_intents(true);
            tree.set_application_scope(Some(application_scope.clone()));
            root(app_tree);
            app_tree.publish_application_scope(&application_scope);
        }
        // The shortcuts the application menu's rows name, before the
        // window's root builds, as a window of a running application gets
        // them.
        let mut app_shortcuts = crate::application_menu::MirroredShortcuts::default();
        if let Some(app_tree) = app_tree.as_mut() {
            sync_headless_shortcuts(app_tree, &mut tree, &mut app_shortcuts);
        }

        // Build the root from the `initial_window`'s builder if one was
        // provided. Headless apps without an `initial_window` run with an
        // empty tree — tests add widgets via `tree.add(...)` directly.
        if let Some(mut config) = self.initial_window.take()
            && let Some(root_builder) = config.take_root_builder()
        {
            // Headless has no real WindowState; construct a stub so
            // widgets that bind against their own window signals
            // still get a valid handle.
            let stub_state = teksilo_core::WindowState::new(teksilo_core::WindowStateInit {
                id: crate::TeksiloWindowId::new(0),
                string_id: config.string_id.clone(),
                placement: config.initial_placement,
                title: config.title.clone(),
                size: config.size,
                position: config.position.unwrap_or((0, 0)),
                focused: true,
                resizable: config.resizable,
                always_on_top: config.always_on_top,
            });
            tree.set_window_state(stub_state.clone());
            root_builder(&mut tree, stub_state);
        }

        HeadlessApp {
            tree,
            app_tree,
            theme: self.theme,
            i18n_manager,
            settings: opened_settings,
            application_scope,
            app_shortcuts,
        }
    }

    /// Build and run the application with windowed rendering, until its
    /// event loop ends: a quit completed, or the
    /// [`QuitPolicy`] saying so with no window open.
    ///
    /// As the loop ends the settings are written and the application's
    /// state is dropped: the widget trees, the handlers given to the
    /// builder, and every service in the app state, whose `Drop` runs then.
    ///
    /// On macOS this may not return. When AppKit carries out a quit (the
    /// Dock menu's Quit, AppleScript's `quit app`, a `terminate:`, a
    /// logout), it ends the process once Teksilo has answered that the quit
    /// completed, and the code after `run()` does not run. Do the work of
    /// the way out in a close guard,
    /// [`on_quit_requested`](Self::on_quit_requested), or the `Drop` of a
    /// service registered with [`app_state`](Self::app_state).
    pub fn run(self) {
        self.run_with(
            || {
                winit::event_loop::EventLoop::<AppEvent>::with_user_event()
                    .build()
                    .expect("winit event loop creation failed")
            },
            |event_loop, app| match event_loop.run_app(app) {
                Ok(()) => {}
                Err(err) if display_connection_lost(&err) => {
                    // Not a fault of this application, and emphatically not a
                    // crash: the compositor exited or died underneath it.
                    // Panicking here produced a stack trace, a crash report
                    // and a bug filed against the renderer, for an event the
                    // application had no part in.
                    eprintln!("teksilo-app: the display server went away ({err}); shutting down");
                }
                Err(err) => panic!("winit event loop exited with error: {err:?}"),
            },
        );
    }

    /// The whole of [`Self::run`] except how the loop is built and how the
    /// finished handler is driven.
    ///
    /// Those two are parameters for exactly one reason: nothing else in the
    /// workspace can construct an `&ActiveEventLoop`, so nothing else can
    /// witness a single one of `TeksiloAppHandler`'s winit callbacks. A test
    /// supplies an off-main-thread loop (`with_any_thread`) and a
    /// `run_app_on_demand` driver that wraps the handler in a script; the
    /// production path supplies `EventLoop::build` and `run_app`, which never
    /// returns. See `crate::app::winit_loop_tests`.
    fn run_with(
        mut self,
        build_loop: impl FnOnce() -> winit::event_loop::EventLoop<AppEvent>,
        drive: impl FnOnce(winit::event_loop::EventLoop<AppEvent>, &mut TeksiloAppHandler),
    ) {
        // Install the tooltip registry before the window manager
        // starts building trees — rich tooltips read from it during
        // their first build.
        if !self.tooltip_contents.is_empty() {
            teksilo_widgets::tooltip::install_tooltip_registry(std::mem::take(
                &mut self.tooltip_contents,
            ));
        }

        // Open settings (if a bundle was configured) so the services
        // are present in the app_state registry when window trees
        // start being built. The `OpenedSettings` handle is kept on
        // the stack so its inner `SettingsFile` clones live long
        // enough to flush on shutdown.
        let opened_settings = self.install_settings();

        // Open telemetry (if a bundle was configured). Must come after
        // install_settings — TelemetryBundle reads the endpoint-override
        // key from the SettingsStore.
        #[cfg(feature = "telemetry")]
        self.install_telemetry(opened_settings.as_ref().map(|s| &s.store));

        // Construct the i18n manager (if configured) and install it on
        // the thread-local before any window or widget tree is created.
        // `WindowManager::create_window` seeds every new tree from the
        // thread-local, so each window inherits the manager's active
        // locale and layout direction on construction — no separate
        // post-create seeding step needed here.
        //
        // `runtime_override` entries are collected before the install
        // so the hot-reload watcher can be spun up after the winit
        // event loop exists (we need the `EventLoopProxy` as the sink
        // target) without a second borrow of `self.i18n`.
        let runtime_overrides: Vec<(LanguageIdentifier, std::path::PathBuf)> = self
            .i18n
            .as_ref()
            .map(|cfg| cfg.runtime_overrides().to_vec())
            .unwrap_or_default();

        if let Some(cfg) = self.i18n.as_ref() {
            install_i18n_manager(cfg);
        }

        let event_loop = build_loop();
        event_loop.set_control_flow(ControlFlow::Wait);

        // Always create a proxy: it's needed by both `on_ready` (if set)
        // and by the event-source poster (if a source is registered). The
        // proxy is cheap to clone.
        let proxy = AppEventProxy {
            inner: event_loop.create_proxy(),
        };

        // Register the process-wide sink for permanently-discarded
        // `teksilo-settings` writes (F3): a `DebouncedWriter` gave up
        // after `MAX_WRITE_ATTEMPTS` retries, or was dropped at teardown
        // with a write still failing. Previously this only reached an
        // `eprintln!` on the settings crate's own background I/O thread
        // and was otherwise invisible; this posts a typed `AppEvent`
        // through the event loop proxy so it reaches the UI thread like
        // every other backend->UI channel (see `user_event` above).
        let proxy_for_write_failure = proxy.inner.clone();
        teksilo_settings::set_write_failure_sink(std::sync::Arc::new(
            move |path, attempts, dropped_patches, message| {
                let _ = proxy_for_write_failure.send_event(AppEvent::SettingsWriteFailed {
                    path,
                    attempts,
                    dropped_patches,
                    message,
                });
            },
        ));

        // Build the i18n hot-reload watcher if any `runtime_override`s
        // were registered. The sink posts `AppEvent::I18nReload` through
        // the event loop proxy; the watcher's background thread converts
        // file-change events into these messages. The watcher handle is
        // handed to `TeksiloAppHandler` which keeps it alive for the loop
        // lifetime. Construction failures log and fall back to no
        // hot-reload (the rest of i18n still works).
        let i18n_watcher = if runtime_overrides.is_empty() {
            None
        } else {
            let proxy_for_sink = proxy.inner.clone();
            let sink: teksilo_i18n::ReloadSink = std::sync::Arc::new(move |locale, path| {
                let _ = proxy_for_sink.send_event(AppEvent::I18nReload {
                    locale: locale.to_string(),
                    path,
                });
            });
            match teksilo_i18n::FtlFileWatcher::new(runtime_overrides, sink) {
                Ok(watcher) => Some(watcher),
                Err(e) => {
                    eprintln!("teksilo-app: failed to start i18n file watcher: {e}");
                    None
                }
            }
        };

        // Build the live cross-process settings-reload watcher, mirroring
        // the i18n watcher immediately above: on by default whenever a
        // settings bundle was actually opened (`opened_settings.is_some()`),
        // opt-out via `.settings_watch(false)`. The sink posts
        // `AppEvent::SettingsReload` through the event loop proxy; the
        // handler (see `user_event` above) dispatches the changed path
        // through the app's `SettingsRegistry` (installed into `app_state`
        // by `install_settings`). Construction failures log and fall back
        // to no live reload — the rest of settings persistence still
        // works, peers just won't be noticed until this process happens
        // to touch the same key itself.
        let settings_watcher = if self.settings_watch_enabled && opened_settings.is_some() {
            self.app_paths.as_ref().and_then(|paths| {
                let proxy_for_sink = proxy.inner.clone();
                let sink: teksilo_settings::SettingsReloadSink = std::sync::Arc::new(move |path| {
                    let _ = proxy_for_sink.send_event(AppEvent::SettingsReload { path });
                });
                let dirs = vec![
                    paths.config_dir().to_path_buf(),
                    paths.data_dir().to_path_buf(),
                ];
                match teksilo_settings::SettingsWatcher::new(dirs, sink) {
                    Ok(watcher) => Some(watcher),
                    Err(e) => {
                        eprintln!("teksilo-app: failed to start settings file watcher: {e}");
                        None
                    }
                }
            })
        } else {
            None
        };

        // Build the typesetter first so we can auto-register it into
        // the per-tree app-state registry below. This gives rich-text
        // widgets (and anything else that needs direct typesetter
        // access) a reachable handle via `ctx.app_state::<SharedTypesetter>()`
        // without forcing the application author to wire it manually.
        let typesetter = self
            .typesetter
            .unwrap_or_else(SharedTypesetter::new_with_default_font);

        // Install app/theme fonts before any text is shaped.
        for registrar in &self.font_registrars {
            typesetter.apply_font_registrar(registrar.as_ref());
        }

        self.app_state_registry.insert(
            std::any::TypeId::of::<SharedTypesetter>(),
            Box::new(typesetter.clone()),
        );
        // Every application has an application menu's holder, empty until
        // the application tree declares one.
        self.app_state_registry
            .entry(std::any::TypeId::of::<teksilo_widgets::menu::ApplicationMenu>())
            .or_insert_with(|| Box::new(teksilo_widgets::menu::ApplicationMenu::new()));

        // Auto-install a system clipboard handle so `RichTextEditor::editor`
        // (and any future clipboard-aware widget) can reach it via
        // `EventContext::app_state::<ClipboardHandle>()`. Behind the
        // `clipboard` feature because it pulls `arboard` into the build.
        // Falls back to `MemoryClipboard` if the OS backend fails to
        // initialize (headless CI, missing display, …) so the editor
        // still works in-process.
        #[cfg(feature = "clipboard")]
        {
            use std::any::TypeId;
            use teksilo_platform::clipboard::{ArboardClipboard, ClipboardHandle, MemoryClipboard};
            let handle = match ArboardClipboard::new() {
                Ok(backend) => ClipboardHandle::new(backend),
                Err(_) => ClipboardHandle::new(MemoryClipboard::new()),
            };
            self.app_state_registry
                .insert(TypeId::of::<ClipboardHandle>(), Box::new(handle));
        }

        // Always build the per-tree app context — the poster is cheap
        // and lets background-work integrations (file dialogs, future
        // async-result features) reach the event loop without forcing
        // an event-source registration. Apps without an event source,
        // app-state registry, or background-work feature simply pay an
        // unused Arc<AppEventPoster> per tree.
        let poster: std::sync::Arc<dyn AppEventPoster> = std::sync::Arc::new(proxy.clone());
        let base = match self.event_source {
            Some(adapter) => TreeAppContext::with_source_and_poster(adapter, poster.clone()),
            None => TreeAppContext::empty(),
        };
        let app_context_template = Some(std::rc::Rc::new(
            base.with_app_state(self.app_state_registry)
                .with_poster(poster),
        ));

        for on_ready in self.on_ready {
            on_ready(proxy.clone());
        }

        let mut app = TeksiloAppHandler::new(
            self.theme,
            self.theme_mode,
            self.app_event_handler,
            self.initial_window,
            self.quit_policy,
            app_context_template,
            typesetter,
            i18n_watcher,
            settings_watcher,
            proxy.clone(),
        );
        // Hand over the app's own ops-bearing external-event router, if any —
        // moved onto the handler after construction (like `loop_tick` below)
        // rather than threaded through `TeksiloAppHandler::new`'s already long
        // parameter list.
        app.wm.set_pen_batching(self.pen_batching);
        app.wm.set_native_theme(self.native_theme);
        app.wm.set_records_announcements(self.records_announcements);
        app.wm.set_app_quit_guard(self.quit_guard);
        app.settings = opened_settings;
        app.termination_signals = self.termination_signals;
        app.external_ctx_handler = self.external_ctx_handler;
        // After the app context, typesetter and theme mode are in place, so
        // the tree is seeded like a window's.
        app.wm.install_app_tree(
            self.app_root
                .unwrap_or_else(|| Box::new(|tree: &mut WidgetTree| tree.add(AppTreeAnchor))),
        );
        // Hand over any registered loop-tick hook (e.g. the `teksilo-async`
        // executor poll). Async-agnostic: just a closure + a poll flag.
        app.loop_tick = self.loop_tick;
        app.loop_tick_poll = self.loop_tick_poll;

        drive(event_loop, &mut app);
        // `exiting` has disconnected every window's waker: winit calls it
        // before its loop ends. Again here, idempotently, so that no window
        // leaves `run_with` connected under a driver that never calls it;
        // such a driver's windows are exposed from the end of its loop to
        // this line.
        app.wm.disconnect_redraw_wakers();

        // Flush any pending settings writes synchronously before the
        // process exits. `exiting` flushed already; this catches what the
        // windows' teardown above wrote since, and a driver that never
        // calls `exiting`. The `DebouncedWriter` background threads also
        // flush on Drop, but doing it synchronously here also surfaces
        // any I/O errors to stderr.
        app.flush_settings();
    }
}

/// Write every pending settings change to disk now, on a way out of the
/// application: the bundle's services, when the application has a bundle
/// (`settings`), then every other settings file of the process (a
/// `SettingsFile`, `MruList` or store the application opened itself),
/// bundle or not.
pub(crate) fn flush_settings_on_exit(settings: Option<&teksilo_settings::OpenedSettings>) {
    let result = match settings {
        Some(opened) => opened.flush_all().map_err(|e| e.to_string()),
        None => teksilo_settings::flush_every_writer().map_err(|e| e.to_string()),
    };
    if let Err(e) = result {
        eprintln!("teksilo-app: settings flush on exit failed: {e}");
    }
}

#[cfg(test)]
mod flush_on_exit_tests {
    use std::time::Duration;
    use teksilo_settings::{AppPaths, SettingsKey, SettingsStore};

    const KEPT: SettingsKey<String> = SettingsKey::new("test.kept", String::new);

    /// An application with no `SettingsBundle` still has the settings files
    /// it opened itself written on the way out: a quit that overrules the
    /// guards, or AppKit ending the process, may never drop them.
    #[test]
    fn an_application_without_a_bundle_has_its_own_files_written() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::for_testing(dir.path());
        let own =
            SettingsStore::open_with_delay(paths.config_file("own"), Duration::from_secs(3600))
                .unwrap();
        own.signal_for(&KEPT).set("written on exit".to_string());

        super::flush_settings_on_exit(None);

        let on_disk = std::fs::read_to_string(paths.config_file("own")).unwrap_or_default();
        assert!(on_disk.contains("written on exit"), "on disk: {on_disk:?}");
        drop(own);
    }
}

// Linux-only: it reaches for winit's X11 extension traits (`with_x11`,
// `with_any_thread`), which exist on the free-unix backends and nowhere else.
// The Windows and macOS jobs therefore never build it, which is also why the
// claims below can only ever be judged against what a Linux host answers.
#[cfg(all(test, target_os = "linux"))]
mod winit_loop_tests;

#[cfg(test)]
mod application_menu_tests;

impl Default for TeksiloAppBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Build an `I18nManager` from `cfg`, pre-resolve its initial locale,
/// and install it on the thread-local. Shared by `build_headless` and
/// `run` so both paths use identical setup. Returns the manager so the
/// headless caller can hand it to `HeadlessApp`; in the windowed `run`
/// path the thread-local owns it for the process lifetime.
fn install_i18n_manager(cfg: &I18nConfig) -> Rc<I18nManager> {
    let mgr = I18nManager::from_config(cfg);
    let initial_loc = I18nManager::resolve_initial_locale(cfg);
    mgr.set_locale(initial_loc);
    teksilo_i18n::thread_local::install(mgr.clone());
    mgr
}

/// Headless-only helper: install the i18n manager AND seed the single
/// `WidgetTree` with the resolved locale and direction. The windowed
/// path doesn't need this because `WindowManager::create_window` reads
/// the thread-local and seeds each new tree at construction time; the
/// headless path has no WindowManager so it seeds its one tree here.
fn install_i18n(tree: &mut WidgetTree, cfg: &I18nConfig) -> Rc<I18nManager> {
    let mgr = install_i18n_manager(cfg);
    tree.set_locale(mgr.locale_signal().get().to_string());
    tree.set_layout_direction(mgr.direction_signal().get());
    mgr
}

/// A headless app for testing (no window, no GPU).
pub struct HeadlessApp {
    pub tree: WidgetTree,
    /// The application tree, when [`TeksiloAppBuilder::app_root`] gave it a
    /// root. Drive it as a running application would: its handlers through
    /// `run_with_event_context`, its mount actions through
    /// `run_mount_actions`, and the intents crossing between it and
    /// [`tree`](Self::tree) through
    /// [`route_unclaimed_intents`](Self::route_unclaimed_intents).
    pub app_tree: Option<WidgetTree>,
    pub theme: Theme,
    /// Active i18n manager, if `TeksiloAppBuilder::i18n(...)` was used. Tests
    /// can reach the bundles, version signal, and locale signal directly
    /// through this handle.
    pub i18n_manager: Option<Rc<I18nManager>>,
    /// Active persistence services, if `TeksiloAppBuilder::settings(...)`
    /// was used. Held here so the underlying `SettingsFile` clones
    /// (and their I/O threads) live as long as the headless app.
    pub settings: Option<teksilo_settings::OpenedSettings>,
    /// What the application tree takes, read by [`tree`](Self::tree)'s
    /// shortcuts. Published when the application tree is built and by
    /// [`route_unclaimed_intents`](Self::route_unclaimed_intents).
    application_scope: teksilo_core::ApplicationScope,
    /// [`tree`](Self::tree)'s copies of the shortcuts the application
    /// menu's rows name, kept in line by
    /// [`route_unclaimed_intents`](Self::route_unclaimed_intents).
    app_shortcuts: crate::application_menu::MirroredShortcuts,
}

/// Bring a headless app's window tree's copies of the application menu's
/// shortcuts in line with `app_tree`'s declarations, after taking a rebind
/// of a copy made in the window to `app_tree`, as a running application
/// does.
fn sync_headless_shortcuts(
    app_tree: &mut WidgetTree,
    tree: &mut WidgetTree,
    mirrored: &mut crate::application_menu::MirroredShortcuts,
) {
    let Some(menu) = app_tree
        .app_context()
        .app_state::<teksilo_widgets::menu::ApplicationMenu>()
        .cloned()
    else {
        return;
    };
    crate::application_menu::sync_all(
        menu.revision(),
        || menu.shortcut_ids(),
        app_tree.shortcut_registry_mut(),
        &mut [(mirrored, tree.shortcut_registry_mut())],
    );
}

impl HeadlessApp {
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Offer what [`tree`](Self::tree) and the
    /// [application tree](Self::app_tree) left unclaimed to each other, as a
    /// running application does after every dispatch, until neither leaves
    /// anything (at most 16 rounds). Returns the names nobody took. Without
    /// an application tree nothing is kept, and this returns nothing.
    ///
    /// A shortcut pressed in `tree` whose intent nothing there takes is
    /// kept when the application tree takes it, as it was when this last
    /// ran (or when the application tree was built); this publishes what
    /// the application tree takes again once it is done.
    ///
    /// A row of the [application menu](teksilo_widgets::menu::ApplicationMenuExt)
    /// chosen in `tree` whose closure belongs to the application tree runs
    /// first: the closure in the application tree, then the row's intent
    /// from `tree`'s focus. Last, `tree`'s copies of the shortcuts the
    /// application menu names are brought in line with it.
    ///
    /// The window operations are a no-op sink, as everywhere in a headless
    /// app.
    pub fn route_unclaimed_intents(&mut self) -> Vec<&'static str> {
        let Some(app_tree) = self.app_tree.as_mut() else {
            return Vec::new();
        };
        let mut dropped = Vec::new();
        let mut ops = teksilo_core::NoopWindowOps;
        if let Some(menu) = app_tree
            .app_context()
            .app_state::<teksilo_widgets::menu::ApplicationMenu>()
            .cloned()
        {
            for row in menu.take_pending() {
                crate::native_menu_route::fire_window_menu_row(
                    app_tree,
                    &mut ops,
                    row.action.as_ref(),
                    None,
                );
                crate::native_menu_route::fire_window_menu_row(
                    &mut self.tree,
                    &mut ops,
                    None,
                    row.intent,
                );
            }
        }
        for _ in 0..16 {
            let from_window = self.tree.take_unclaimed_intents();
            let from_app = app_tree.take_unclaimed_intents();
            if from_window.is_empty() && from_app.is_empty() {
                break;
            }
            dropped.extend(crate::intent_routing::offer(
                from_window,
                Some(app_tree),
                &mut ops,
            ));
            dropped.extend(crate::intent_routing::offer(
                from_app,
                Some(&mut self.tree),
                &mut ops,
            ));
        }
        app_tree.publish_application_scope(&self.application_scope);
        sync_headless_shortcuts(app_tree, &mut self.tree, &mut self.app_shortcuts);
        dropped
    }

    /// The active i18n manager, if `i18n(...)` was registered on the
    /// builder.
    pub fn i18n_manager(&self) -> Option<&Rc<I18nManager>> {
        self.i18n_manager.as_ref()
    }

    /// Switch the active locale. Updates the manager (which increments the
    /// version signal so any `LocalizedString::to_signal()` observers
    /// re-resolve), then seeds the tree with the new direction (only when
    /// it actually changed) and triggers a composite rebuild via
    /// `WidgetTree::set_locale`. No-op if no `I18nConfig` was registered.
    pub fn set_locale(&mut self, locale: LanguageIdentifier) {
        let Some(mgr) = self.i18n_manager.clone() else {
            return;
        };
        let outcome = mgr.set_locale(locale.clone());
        if outcome.direction_changed {
            self.tree.set_layout_direction(mgr.direction_signal().get());
        }
        self.tree.set_locale(locale.to_string());
    }

    /// Render the tree for an offscreen capture through `renderer`, with
    /// the glyph atlas its text samples uploaded first.
    ///
    /// `Renderer::render` does not upload the atlas, so a capture that only
    /// calls `tree.render()` draws its text blank. `atlas_version` is the
    /// atlas version `renderer` last received: start it at 0 for a fresh
    /// renderer, and keep it with that renderer across calls so an
    /// unchanged atlas is not uploaded again. After an eviction the tree's
    /// retained paints point at recycled atlas slots, so it is repainted
    /// before the upload.
    pub fn render_for_capture(
        &mut self,
        renderer: &mut teksilo_render::Renderer,
        atlas_version: &mut u64,
    ) -> Rc<teksilo_canvas::RenderFrame> {
        let frame = self.tree.render();
        let Some(typesetter) = self
            .tree
            .app_context()
            .app_state::<SharedTypesetter>()
            .cloned()
        else {
            return frame;
        };
        if sync_glyph_atlas(&typesetter, renderer, atlas_version) {
            typesetter.bridge().borrow_mut().invalidate_cache();
            self.tree.invalidate_all_paints();
            drop(frame);
            let frame = self.tree.render();
            let evicted_again = sync_glyph_atlas(&typesetter, renderer, atlas_version);
            debug_assert!(
                !evicted_again,
                "glyph eviction during eviction recovery — epoch delta would be lost"
            );
            return frame;
        }
        frame
    }
}

/// Upload the typesetter's glyph atlas into `renderer` when it changed since
/// `uploaded_version`, and report whether glyphs were evicted since the
/// previous query. The caller owns the eviction recovery: what must be
/// repainted depends on how many trees share the typesetter.
pub(crate) fn sync_glyph_atlas(
    typesetter: &SharedTypesetter,
    renderer: &mut teksilo_render::Renderer,
    uploaded_version: &mut u64,
) -> bool {
    let atlas = typesetter
        .bridge()
        .borrow_mut()
        .atlas_info(*uploaded_version);
    if atlas.version != *uploaded_version && atlas.width > 0 && atlas.height > 0 {
        renderer.upload_atlas(atlas.width, atlas.height, &atlas.pixels);
        *uploaded_version = atlas.version;
    }
    atlas.glyphs_evicted
}

#[cfg(test)]
mod tests {
    use super::*;
    use teksilo_i18n::lit;
    use teksilo_tokens::Color;
    use teksilo_widgets::{Button, ModalContainer};

    #[test]
    fn builder_accepts_theme() {
        let app = TeksiloAppBuilder::new()
            .theme(teksilo_core::presets::intui::light())
            .build_headless();
        assert_ne!(app.theme().colors.accent, Color::TRANSPARENT);
    }

    #[test]
    fn register_post_root_composes_instead_of_clobbering() {
        // Regression: installing two post-root chrome wrappers (e.g. the
        // debug inspector AND the toast host) must run BOTH, not just the
        // last-installed one — `app_state(DefaultPostRoot)` is type-keyed
        // and silently overwrote the earlier hook, killing F12 / overflow
        // stripes in any app that also installed toast.
        use crate::DefaultPostRoot;
        use std::cell::RefCell;
        use std::rc::Rc;

        let order: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
        let (o1, o2) = (order.clone(), order.clone());

        let builder = TeksiloAppBuilder::new()
            .register_post_root(DefaultPostRoot::new(move |_t, id| {
                o1.borrow_mut().push("inspector");
                id
            }))
            .register_post_root(DefaultPostRoot::new(move |_t, id| {
                o2.borrow_mut().push("toast");
                id
            }));

        let composed = builder
            .app_state_registry
            .get(&TypeId::of::<DefaultPostRoot>())
            .and_then(|b| b.downcast_ref::<DefaultPostRoot>())
            .expect("composed DefaultPostRoot must be present")
            .clone();

        let mut tree = WidgetTree::new();
        let root = tree.add(Button::new(lit!("root")));
        let out = (composed.0)(&mut tree, root);

        assert_eq!(
            *order.borrow(),
            vec!["inspector", "toast"],
            "both hooks run, earliest-registered innermost (first)"
        );
        assert_eq!(out, root, "passthrough hooks return the same root id");
    }

    #[test]
    fn register_app_event_observer_composes_instead_of_clobbering() {
        // Mirrors `register_post_root_composes_instead_of_clobbering`
        // above: two extensions each registering their own `AppEvent`
        // observer (e.g. a future telemetry hook AND
        // `teksilo::install_toast`'s settings-write-failure toast) must
        // both fire, not just the last-installed one.
        use crate::app_event_observers::AppEventObservers;
        use std::cell::RefCell;
        use std::rc::Rc;
        use teksilo_core::app_event::AppEvent;

        let order: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
        let (o1, o2) = (order.clone(), order.clone());

        let builder = TeksiloAppBuilder::new()
            .register_app_event_observer(move |_event| {
                o1.borrow_mut().push("first");
            })
            .register_app_event_observer(move |_event| {
                o2.borrow_mut().push("second");
            });

        let composed = builder
            .app_state_registry
            .get(&TypeId::of::<AppEventObservers>())
            .and_then(|b| b.downcast_ref::<AppEventObservers>())
            .expect("composed AppEventObservers must be present")
            .clone();

        let event = AppEvent::BackgroundComplete {
            operation_id: "op".to_string(),
        };
        (composed.0)(&event);

        assert_eq!(
            *order.borrow(),
            vec!["first", "second"],
            "both observers run, in registration order"
        );
    }

    #[test]
    fn register_app_event_observer_does_not_suppress_on_app_event_handler() {
        // The composable observer slot and the single `on_app_event`
        // handler slot are independent storage (`app_state_registry` vs
        // `app_event_handler`), so registering one must never clear or
        // shadow the other. `TeksiloAppHandler::user_event` dispatches
        // both (handler first, then composed observers) — this test
        // proves the two slots coexist and mirrors that dispatch order
        // directly, since driving the real `ApplicationHandler::user_event`
        // requires a live winit event loop unavailable in a unit test.
        use crate::app_event_observers::AppEventObservers;
        use std::cell::RefCell;
        use std::rc::Rc;
        use teksilo_core::app_event::AppEvent;

        let handler_fired = Rc::new(RefCell::new(false));
        let observer_fired = Rc::new(RefCell::new(false));
        let (h1, h2) = (handler_fired.clone(), observer_fired.clone());

        let mut builder = TeksiloAppBuilder::new()
            .on_app_event(move |_event| {
                *h1.borrow_mut() = true;
            })
            .register_app_event_observer(move |_event| {
                *h2.borrow_mut() = true;
            });

        let mut handler = builder
            .app_event_handler
            .take()
            .expect("on_app_event handler must survive register_app_event_observer");
        let observers = builder
            .app_state_registry
            .get(&TypeId::of::<AppEventObservers>())
            .and_then(|b| b.downcast_ref::<AppEventObservers>())
            .expect("registered observer must survive on_app_event")
            .clone();

        let event = AppEvent::BackgroundComplete {
            operation_id: "op".to_string(),
        };
        // Mirrors the dispatch order in `user_event`: handler first, then
        // composed observers.
        handler(&event);
        (observers.0)(&event);

        assert!(
            *handler_fired.borrow(),
            "on_app_event's handler must still fire"
        );
        assert!(
            *observer_fired.borrow(),
            "the registered observer must also fire"
        );
    }

    /// `on_external_with_ctx` is a third, independent slot: registering it must
    /// not disturb `on_app_event`'s handler or the composable observers, and
    /// they must not disturb it. Same shape (and same limitation) as the test
    /// above — driving the real `ApplicationHandler::user_event` needs a live
    /// winit event loop, so this proves slot independence and the router's own
    /// claim contract; that it truly receives a window-capable `EventContext`
    /// is proven end-to-end by Skribisto's `scripts/automation_single_instance.py`.
    #[test]
    fn on_external_with_ctx_is_a_slot_of_its_own() {
        use crate::app_event_observers::AppEventObservers;

        let builder = TeksiloAppBuilder::new()
            .on_external_with_ctx(|_payload, _ctx| true)
            .on_app_event(|_event| {})
            .register_app_event_observer(|_event| {});

        assert!(
            builder.external_ctx_handler.is_some(),
            "the external router must survive a later on_app_event/observer registration"
        );
        assert!(
            builder.app_event_handler.is_some(),
            "on_app_event must survive on_external_with_ctx"
        );
        assert!(
            builder
                .app_state_registry
                .contains_key(&TypeId::of::<AppEventObservers>()),
            "observers must survive on_external_with_ctx"
        );

        // Single slot, like `on_app_event`: registering twice replaces.
        let builder = builder.on_external_with_ctx(|_payload, _ctx| false);
        let mut router = builder
            .external_ctx_handler
            .expect("the second registration is the live one");
        // Exercise the claim contract — the `bool` `user_event` branches on to
        // decide whether the payload was the app's — through a real, headless
        // `EventContext`. `NoopWindowOps` is the same sink `window_manager`'s
        // own close-guard tests use; the live `WindowOpsImpl` only arrives with
        // a winit event loop.
        let mut tree = teksilo_core::WidgetTree::new();
        let mut claimed = true;
        tree.run_with_event_context(
            &mut teksilo_core::NoopWindowOps,
            |ctx: &mut teksilo_core::widget::EventContext| {
                claimed = router(&42i32, ctx);
            },
        );
        assert!(
            !claimed,
            "the replacing router's answer is the one that decides"
        );
    }

    #[test]
    fn builder_with_root() {
        use teksilo_widgets::RectWidget;
        let app = TeksiloAppBuilder::new()
            .initial_window(
                WindowConfig::new()
                    .root(|tree, _state| tree.add(RectWidget::new().background(Color::RED))),
            )
            .build_headless();
        let mut tree = app.tree;
        tree.layout(SizeProposal::exact(200.0, 100.0));
        let frame = tree.render();
        assert!(!frame.is_empty());
    }

    #[test]
    fn app_state_flows_through_headless_builder() {
        use std::rc::Rc;
        use teksilo_core::build_context::BuildContext;
        use teksilo_core::signal::Signal;
        use teksilo_core::widget::{LayoutContext, Widget};

        struct AppGlobals {
            label: Signal<String>,
        }

        #[derive(Debug)]
        struct GlobalsReader {
            observed: Signal<String>,
        }

        impl Widget for GlobalsReader {
            fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
                let globals = ctx
                    .app_state::<Rc<AppGlobals>>()
                    .expect("AppGlobals not registered");
                self.observed.set(globals.label.get());
                Vec::new()
            }

            fn layout_response(
                &self,
                proposal: SizeProposal,
                _ctx: &LayoutContext,
            ) -> teksilo_core::widget::LayoutResponse {
                proposal.resolve(0.0, 0.0).into()
            }
        }

        let globals = Rc::new(AppGlobals {
            label: Signal::new("headless works".to_string()),
        });

        let observed = Signal::new(String::new());
        let observed_for_root = observed.clone();

        let _app = TeksiloAppBuilder::new()
            .app_state(globals.clone())
            .initial_window(WindowConfig::new().root(move |tree, _state| {
                tree.add(GlobalsReader {
                    observed: observed_for_root.clone(),
                })
            }))
            .build_headless();

        assert_eq!(observed.get(), "headless works");
    }

    #[test]
    fn auto_prefers_native_for_deferred_content_when_supported() {
        let request = ModalRequest::deferred(|tree| tree.add(Button::new(lit!("Deferred"))));

        assert_eq!(
            resolve_modal_presentation(request.presentation, &request.content, true),
            ResolvedModalPresentation::NativeWindow
        );
    }

    #[test]
    fn a_native_modal_honours_the_escape_half_of_its_close_behavior() {
        // The whole point of finding 1: `close_behavior` used to fall into the
        // `ModalRequest` destructure's `..` in the NativeWindow arm and was
        // never read, so the DEFAULT behaviour on the DEFAULT presentation did
        // nothing. This is the table that arm now consults.
        assert!(native_modal_escape_dismisses(
            ModalCloseBehavior::EscapeOrClickOutside
        ));
        assert!(native_modal_escape_dismisses(ModalCloseBehavior::EscapeKey));
        // No "outside" exists while the OS blocks the parent, so these two
        // agree — deliberately, and not by omission.
        assert!(!native_modal_escape_dismisses(
            ModalCloseBehavior::ClickOutside
        ));
        assert!(!native_modal_escape_dismisses(ModalCloseBehavior::Manual));
    }

    #[test]
    fn the_native_modal_escape_wrapper_asks_to_dismiss_the_window() {
        // The wrapper the NativeWindow arm splices around the modal's root:
        // an unclaimed Escape reaching it must request the window's dismissal.
        let mut tree = WidgetTree::new();
        let content = tree.add(Button::new(lit!("Inside")));
        let _root = wrap_native_modal_in_escape_route(&mut tree, content);
        tree.layout(teksilo_canvas::SizeProposal::exact(300.0, 200.0));
        tree.focus(content);
        assert!(!tree.has_pending_modal_dismissal());

        tree.press_key(
            teksilo_core::event::Key::Escape,
            teksilo_core::event::Modifiers::NONE,
        );

        assert!(
            tree.has_pending_modal_dismissal(),
            "an unclaimed Escape inside a native modal must ask to close its window"
        );
    }

    /// A `MessageBox` in a native dialog window asks for 460 wide, the size
    /// its own column is; the dialog panel pads it 24 on each side, so the
    /// window must grow to 508 wide or the box starts at x = 0, its buttons
    /// against the window's edges. Built as `open_modal_window` builds it,
    /// sized as the frame loop sizes it. With the width left fixed the box
    /// sits at x = 0.
    #[test]
    fn a_native_message_box_window_is_as_wide_as_its_padded_content() {
        use teksilo_widgets::{EventContextMessageBoxExt, MessageBox, MessageBoxButtons};
        let theme = teksilo_core::presets::intui::light();
        let mut asking = WidgetTree::new().with_theme(theme.clone());
        let ask = asking.add(Button::new(lit!("Close")).on_activate_fn(|ctx| {
            ctx.present_message_box(
                MessageBox::question(lit!("Save the changes before closing?"))
                    .buttons(MessageBoxButtons::OkCancel),
            );
        }));
        asking.layout(teksilo_canvas::SizeProposal::exact(200.0, 100.0));
        asking.click(ask);
        let request = asking
            .drain_pending_modal_requests()
            .pop()
            .expect("a modal request")
            .request;
        let floor = request.size.expect("a requested size");
        let ModalContent::Deferred(builder) = request.content else {
            panic!("deferred content");
        };
        let mut tree = WidgetTree::new().with_theme(theme);
        let content = builder(&mut tree);
        wrap_native_modal_in_escape_route(&mut tree, content);

        let mut size = floor;
        for _ in 0..4 {
            tree.layout(teksilo_canvas::SizeProposal::exact(
                size.0 as f32,
                size.1 as f32,
            ));
            match size_to_content_target(&tree, (size.0 as f32, size.1 as f32), Some(floor.0)) {
                Some(target) if target != size => size = target,
                _ => break,
            }
        }
        tree.layout(teksilo_canvas::SizeProposal::exact(
            size.0 as f32,
            size.1 as f32,
        ));
        let mut stack = tree.roots();
        let mut message_box = None;
        while let Some(id) = stack.pop() {
            if tree
                .widget_type_name(id)
                .is_some_and(|name| name.ends_with("MessageBox"))
            {
                message_box = Some(id);
                break;
            }
            stack.extend(tree.children(id));
        }
        let bounds = tree.bounds(message_box.expect("the MessageBox"));
        assert!(
            bounds.x >= 24.0 - 0.5 && bounds.x + bounds.width <= size.0 as f32 - 24.0 + 0.5,
            "the box inside the panel's padding: {bounds:?} in a window {size:?}"
        );
        assert!(
            size.0 >= floor.0 && size.1 >= 1,
            "never below the request: {size:?}"
        );
    }

    #[test]
    fn existing_widget_forces_in_tree_even_if_native_requested() {
        let mut tree = WidgetTree::new();
        let content = tree.add(Button::new(lit!("Existing")));
        let request = ModalRequest::in_tree(content).presentation(ModalPresentation::NativeWindow);

        assert_eq!(
            resolve_modal_presentation(request.presentation, &request.content, true),
            ResolvedModalPresentation::InTree
        );
    }

    #[test]
    fn present_in_tree_modal_request_shows_centered_overlay() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        let content = tree.add(Button::new(lit!("Modal content")));
        tree.set_dormant(content);
        tree.layout(SizeProposal::exact(800.0, 600.0));

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::in_tree(content).presentation(ModalPresentation::InTree),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        // Two overlays: the modal-panel overlay AND the dialog scrim
        // pushed below it by the modal-presentation pipeline.
        assert_eq!(tree.active_overlays().len(), 2);
        assert!(tree.find_by_label("Modal content").is_some());
    }

    #[test]
    fn present_in_tree_modal_request_honours_a_top_placement() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        let content = tree.add(Button::new(lit!("Modal content")));
        tree.set_dormant(content);
        tree.layout(SizeProposal::exact(800.0, 600.0));

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::in_tree(content)
                .presentation(ModalPresentation::InTree)
                .placement(teksilo_core::modal::ModalPlacement::Top { offset: 64.0 }),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let bounds = tree.bounds(content);
        assert_eq!(bounds.y, 64.0, "the top edge sits at the requested offset");
        assert!(
            (bounds.x + bounds.width / 2.0 - 400.0).abs() < 0.5,
            "and the modal is centred horizontally: {bounds:?}"
        );
    }

    #[test]
    fn present_in_tree_modal_request_builds_deferred_content() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(|tree| tree.add(Button::new(lit!("Deferred modal"))))
                .presentation(ModalPresentation::InTree),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        // Two overlays: the modal-panel overlay AND the dialog scrim
        // pushed below it by the modal-presentation pipeline.
        assert_eq!(tree.active_overlays().len(), 2);
        assert!(tree.find_by_label("Deferred modal").is_some());
    }

    #[test]
    fn present_in_tree_modal_request_mounts_scrim_below_modal() {
        // The scrim must be pushed BEFORE the modal so it z-orders
        // below the panel. `active_content_ids()` returns ids in
        // stack order (oldest → newest), so the first id is the
        // scrim and the second is the modal content.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        let content = tree.add(Button::new(lit!("Modal content")));
        tree.set_dormant(content);
        tree.layout(SizeProposal::exact(800.0, 600.0));

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::in_tree(content).presentation(ModalPresentation::InTree),
        );

        let stack = tree.overlay_manager().active_content_ids();
        assert_eq!(stack.len(), 2, "scrim + modal");
        // Scrim is the first one; modal content the second.
        assert_eq!(stack[1], content, "modal content sits above scrim");
    }

    // ─── A dismissal must report its result ────────────────────────
    //
    // These drive a modal through the REAL in-tree path —
    // `ctx.present_modal` → `drain_pending_modal_requests` →
    // `present_in_tree_modal_request` — because that is the path that is
    // broken and the one nothing covered.
    //
    // Every existing test of this behaviour builds the modal's content
    // straight into the tree and never calls `show_overlay`
    // (`message_box.rs`'s `present_and_lay_out`, and the `InputDialog`
    // tests). With no overlay on the stack, `pointer_router`'s Escape arm
    // is skipped and the widget's own Escape shortcut resolves — which is
    // the NATIVE topology. So the suite has been testing the arm this
    // machine cannot execute, and is green while a Linux user loses their
    // answer: `supports_native_modal_windows()` is false on every unix but
    // macOS, so `Auto` resolves here to the overlay these tests omit.
    //
    // Measured against the live `dialogs-and-popovers` demo: clicking a
    // button reports, Escape does not, clicking outside does not.

    /// Present `mb` the way an app does, dismiss it with `dismiss`, and
    /// return whatever `on_result` was handed — `None` if it never ran.
    fn message_box_dismissed_by(
        mb: teksilo_widgets::MessageBox,
        dismiss: impl FnOnce(&mut WidgetTree),
    ) -> Option<teksilo_widgets::MessageBoxResult> {
        use std::cell::RefCell;
        use std::rc::Rc;

        let seen: Rc<RefCell<Option<teksilo_widgets::MessageBoxResult>>> =
            Rc::new(RefCell::new(None));
        let sink = seen.clone();
        let pending = Rc::new(RefCell::new(Some(mb.on_result(move |r, _ctx| {
            *sink.borrow_mut() = Some(r);
        }))));

        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let trigger = tree.add(Button::new(lit!("Open")).on_activate_fn(move |ctx| {
            if let Some(mb) = pending.borrow_mut().take() {
                mb.present(ctx);
            }
        }));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        tree.dispatch_event(teksilo_core::event::WidgetEvent::AccessAction {
            action: teksilo_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: teksilo_core::accessibility::root_node_id(),
            data: None,
        });
        let queued = tree
            .drain_pending_modal_requests()
            .pop()
            .expect("MessageBox::present must queue a modal request");
        present_in_tree_modal_request(&mut tree, queued.source_widget, queued.request);
        tree.layout(SizeProposal::exact(800.0, 600.0));
        assert_eq!(
            tree.active_overlays().len(),
            2,
            "precondition: the in-tree arm ran (scrim + panel)"
        );

        dismiss(&mut tree);
        tree.layout(SizeProposal::exact(800.0, 600.0));
        assert_eq!(
            tree.active_overlays().len(),
            0,
            "precondition: the modal actually closed — otherwise a missing result \
             would mean 'still open', which is a different bug"
        );
        *seen.borrow()
    }

    /// A point well outside the centred 460×140 panel in an 800×600
    /// viewport, so a press there lands on the scrim.
    fn click_outside(tree: &mut WidgetTree) {
        use teksilo_core::event::{Modifiers, PointerButton, WidgetEvent};
        let at = teksilo_canvas::Point::new(8.0, 8.0);
        tree.pointer_move(at);
        tree.dispatch_event(WidgetEvent::pointer_down(
            at,
            PointerButton::Primary,
            Modifiers::default(),
        ));
        tree.dispatch_event(WidgetEvent::pointer_up(
            at,
            PointerButton::Primary,
            Modifiers::default(),
        ));
    }

    fn a_question() -> teksilo_widgets::MessageBox {
        use teksilo_widgets::{MessageBox, MessageBoxButtons, StandardButton};
        MessageBox::question(lit!("Save changes?"))
            .buttons(MessageBoxButtons::SaveDiscardCancel)
            .escape_button(StandardButton::Cancel)
    }

    /// The positive control, and the reason the three failures below can be
    /// read as a framework defect rather than a broken rig: identical
    /// harness, identical presentation, identical sink — the only thing
    /// that differs is the route out. This one passes today.
    #[test]
    fn clicking_a_button_on_an_in_tree_message_box_reports_its_result() {
        let seen = message_box_dismissed_by(a_question(), |tree| {
            let cancel = tree
                .find_by_label("Cancel")
                .expect("the Cancel button is in the modal's content");
            tree.dispatch_event(teksilo_core::event::WidgetEvent::AccessAction {
                action: teksilo_core::accesskit::Action::Click,
                target: Some(cancel),
                target_node: teksilo_core::accessibility::root_node_id(),
                data: None,
            });
        })
        .expect("clicking a button reports its result");

        assert_eq!(seen.button, teksilo_widgets::StandardButton::Cancel);
        assert_eq!(
            seen.dismissal,
            teksilo_widgets::MessageBoxDismissal::Button,
            "a deliberate button press is a choice, not a dismissal"
        );
    }

    #[test]
    fn escape_on_an_in_tree_message_box_reports_its_result() {
        let seen = message_box_dismissed_by(a_question(), |tree| {
            tree.press_key(
                teksilo_core::event::Key::Escape,
                teksilo_core::event::Modifiers::NONE,
            );
        });

        let seen = seen.expect(
            "Escape closed the modal but `on_result` never ran, so the app cannot \
             learn the user's answer. `pointer_router`'s overlay Escape arm consumes \
             the key and returns before shortcut resolution, so MessageBox's own \
             Escape shortcut never fires on the in-tree arm",
        );
        assert_eq!(
            seen.button,
            teksilo_widgets::StandardButton::Cancel,
            "Escape must resolve to the declared escape button"
        );
        assert_eq!(
            seen.dismissal,
            teksilo_widgets::MessageBoxDismissal::Escape,
            "and must name the route, not merely report that something happened"
        );
    }

    #[test]
    fn clicking_outside_an_in_tree_message_box_reports_its_result() {
        let seen = message_box_dismissed_by(a_question(), click_outside);

        let seen = seen.expect(
            "a click outside closed the modal but `on_result` never ran. \
             `MessageBoxResult::dismissed_by_escape` is documented as covering \
             scrim-click, and no code path can currently produce it",
        );
        assert_eq!(seen.button, teksilo_widgets::StandardButton::Cancel);
        assert_eq!(
            seen.dismissal,
            teksilo_widgets::MessageBoxDismissal::ClickOutside,
            "a press outside is not Escape, and the caller can now tell"
        );
    }

    /// The page behind an in-tree message box, as a screen reader meets it.
    /// `tools/reader/` (dialogs-08, in `dialogs-and-popovers`): with "Save
    /// changes?" up, an AT-SPI click on the page's "Welcome" opened a second
    /// box over it, and `grab_focus` put focus on "Delete file?" behind both.
    #[test]
    fn a_screen_reader_cannot_act_behind_an_in_tree_message_box() {
        use teksilo_core::accessibility::widget_id_to_node_id;
        use teksilo_core::accesskit::Action;
        use teksilo_core::window::NoopWindowOps;
        use teksilo_widgets::{MessageBox, MessageBoxButtons, VStack};

        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let opener = tree.add(
            Button::new(lit!("Save changes?")).on_activate_fn(|ctx| a_question().present(ctx)),
        );
        let welcome = tree.add(Button::new(lit!("Welcome")).on_activate_fn(|ctx| {
            MessageBox::information(lit!("Welcome to Teksilo"))
                .buttons(MessageBoxButtons::Ok)
                .present(ctx)
        }));
        let delete = tree.add(Button::new(lit!("Delete file?")));
        tree.add(VStack::new().child(opener).child(welcome).child(delete));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        assert!(tree.dispatch_access_action(
            widget_id_to_node_id(opener),
            Action::Click,
            None,
            &mut NoopWindowOps,
        ));
        let queued = tree
            .drain_pending_modal_requests()
            .pop()
            .expect("the opener queues its message box");
        present_in_tree_modal_request(&mut tree, queued.source_widget, queued.request);
        tree.layout(SizeProposal::exact(800.0, 600.0));
        let in_the_box = tree.focused();
        assert!(
            in_the_box.is_some(),
            "precondition: focus went into the box"
        );

        assert!(
            !tree.dispatch_access_action(
                widget_id_to_node_id(welcome),
                Action::Click,
                None,
                &mut NoopWindowOps,
            ),
            "a click behind the box is refused"
        );
        assert!(
            tree.drain_pending_modal_requests().is_empty(),
            "a click behind 'Save changes?' must not open a second box over it"
        );
        assert!(!tree.dispatch_access_action(
            widget_id_to_node_id(delete),
            Action::Focus,
            None,
            &mut NoopWindowOps,
        ));
        assert_eq!(
            tree.focused(),
            in_the_box,
            "focus stays in the box, where the next real key goes"
        );
    }

    /// The menu bar's window-level keys do nothing behind an in-tree message
    /// box: Alt+E opens no menu, and neither it nor F10 moves focus out of the
    /// box. A bar collapsed to its hamburger is the case that needs saying:
    /// its reveal floats the bar as an overlay *above* the box, where a menu
    /// would be in front of the modal and free to act. macOS has no Alt+letter
    /// mnemonics (the OS composes Option+letter first), so there only F10 is
    /// checked.
    #[test]
    fn the_menu_bar_keys_do_nothing_behind_an_in_tree_message_box() {
        use crate::input_routing::apply_menubar_action;
        use crate::input_routing::tests::{menubar_answer, tree_with_menu_bar};
        use teksilo_core::event::{Key, Modifiers};
        use teksilo_core::window::NoopWindowOps;

        for collapsed in [false, true] {
            let (mut tree, menu_bar, proposal) = tree_with_menu_bar(collapsed);
            tree.run_with_event_context(&mut NoopWindowOps, |ctx| a_question().present(ctx));
            let queued = tree
                .drain_pending_modal_requests()
                .pop()
                .expect("the message box is queued");
            present_in_tree_modal_request(&mut tree, queued.source_widget, queued.request);
            tree.layout(proposal);
            let in_the_box = tree.focused();
            assert!(
                in_the_box.is_some(),
                "precondition: focus went into the box"
            );

            let focus_file = menubar_answer(&tree, Key::F10, Modifiers::NONE);
            #[cfg(target_os = "macos")]
            let actions = [focus_file];
            #[cfg(not(target_os = "macos"))]
            let (edit, actions) = {
                use crate::input_routing::tests::{alt_letter, opened_trigger};
                let open_edit = alt_letter(&tree, 'e');
                (opened_trigger(&open_edit), [open_edit, focus_file])
            };
            for action in actions {
                apply_menubar_action(&mut tree, &mut NoopWindowOps, action, proposal);
                tree.layout(proposal);
                tree.advance_time(std::time::Duration::from_secs(1));
                tree.layout(proposal);
                assert_eq!(
                    tree.focused(),
                    in_the_box,
                    "collapsed: {collapsed}, focus stays in the box"
                );
            }
            #[cfg(not(target_os = "macos"))]
            assert!(
                !tree.accessibility_node(edit).is_expanded(),
                "collapsed: {collapsed}, no menu opens over the box"
            );
            if collapsed {
                assert!(
                    !tree.is_active(tree.children(menu_bar)[0]),
                    "the bar is not floated over the box"
                );
            }
        }
    }

    /// `InputDialog` is the worse case: `None` *is* its cancellation
    /// payload, so a dismissal that reports nothing is indistinguishable
    /// from a dialog still sitting open — and unlike `MessageBox` it
    /// registers no Escape route of its own, so this fails on the native
    /// arm too.
    #[test]
    fn escape_on_an_in_tree_input_dialog_reports_its_cancellation() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let seen: Rc<RefCell<Option<Option<String>>>> = Rc::new(RefCell::new(None));
        let sink = seen.clone();
        let pending = Rc::new(RefCell::new(Some(
            teksilo_widgets::InputDialog::new(lit!("Rename")).on_result(move |v, _ctx| {
                *sink.borrow_mut() = Some(v);
            }),
        )));

        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let trigger = tree.add(Button::new(lit!("Open")).on_activate_fn(move |ctx| {
            if let Some(d) = pending.borrow_mut().take() {
                d.present(ctx);
            }
        }));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        tree.dispatch_event(teksilo_core::event::WidgetEvent::AccessAction {
            action: teksilo_core::accesskit::Action::Click,
            target: Some(trigger),
            target_node: teksilo_core::accessibility::root_node_id(),
            data: None,
        });
        let queued = tree
            .drain_pending_modal_requests()
            .pop()
            .expect("InputDialog::present must queue a modal request");
        present_in_tree_modal_request(&mut tree, queued.source_widget, queued.request);
        tree.layout(SizeProposal::exact(800.0, 600.0));

        tree.press_key(
            teksilo_core::event::Key::Escape,
            teksilo_core::event::Modifiers::NONE,
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let reported = seen.borrow().clone().expect(
            "Escape closed the InputDialog but `on_result` never ran. Its own rustdoc \
             says it is 'invoked exactly once when the user accepts (Some) or cancels \
             (None)' — a dismissal is a cancellation, and the caller is left unable to \
             tell it from a dialog that is still open",
        );
        assert!(
            reported.is_none(),
            "a dismissal is a cancellation, so the payload must be None"
        );
    }

    #[test]
    fn dismissing_modal_cascades_to_scrim() {
        // The scrim's `parent_overlay` is patched to the modal id
        // after both are pushed. Dismissing the modal must therefore
        // also dismiss the scrim through the cascade walk in
        // `dismiss_immediate`.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        let content = tree.add(Button::new(lit!("Modal content")));
        tree.set_dormant(content);
        tree.layout(SizeProposal::exact(800.0, 600.0));

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::in_tree(content).presentation(ModalPresentation::InTree),
        );
        assert_eq!(tree.active_overlays().len(), 2);

        // Find the modal's overlay id (the one whose content is the
        // modal content widget) and dismiss it.
        let modal_overlay = tree
            .overlay_manager()
            .find_by_content(content)
            .expect("modal overlay registered");
        tree.overlay_manager_mut().dismiss(modal_overlay);

        assert!(
            tree.active_overlays().is_empty(),
            "scrim must cascade away with the modal",
        );
    }

    #[test]
    fn scrim_uses_full_viewport_placement() {
        // The scrim's overlay placement determines its bounds during
        // `position_overlays`. It must be `FullViewport` so the dim
        // covers the entire window regardless of the modal's size or
        // position.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        let content = tree.add(Button::new(lit!("Modal content")));
        tree.set_dormant(content);
        tree.layout(SizeProposal::exact(800.0, 600.0));

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::in_tree(content).presentation(ModalPresentation::InTree),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        // The scrim is at the bottom of the stack — first content id.
        let scrim_content_id = tree.overlay_manager().active_content_ids()[0];
        let scrim_bounds = tree.bounds(scrim_content_id);
        assert!(
            (scrim_bounds.width - 800.0).abs() < 0.01,
            "scrim spans the viewport width",
        );
        assert!(
            (scrim_bounds.height - 600.0).abs() < 0.01,
            "scrim spans the viewport height",
        );
    }

    #[test]
    fn present_in_tree_modal_request_moves_focus_into_modal() {
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        tree.focus(source);

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(|tree| {
                tree.add(ModalContainer::new(Button::new(lit!("Continue"))))
            })
            .presentation(ModalPresentation::InTree),
        );

        let continue_button = tree.find_by_label("Continue").unwrap();
        assert_eq!(tree.focused(), Some(continue_button));
    }

    /// **A command run from the palette reaches the field it was opened over.**
    ///
    /// The palette closes, focus goes back to the field, and the intent is
    /// dispatched from there, so an action registered around the field
    /// answers it. Before, the intent started from the palette, already
    /// dismissed, and only window-global actions heard it.
    #[test]
    fn a_palette_command_runs_where_the_palette_was_opened() {
        use std::cell::Cell;
        use std::rc::Rc;
        use teksilo_core::event::Key;
        use teksilo_core::shortcut::Shortcut;
        use teksilo_core::signal::Signal;
        use teksilo_widgets::{CommandPalette, TextInput};

        /// Wraps a text field and answers `doc.count_words`, counting runs.
        #[derive(Debug)]
        struct Document {
            runs: Rc<Cell<u32>>,
            child: Option<WidgetId>,
        }
        impl teksilo_core::widget::Widget for Document {
            fn build(&mut self, ctx: &mut teksilo_core::BuildContext) -> Vec<WidgetId> {
                let runs = self.runs.clone();
                ctx.register_action(
                    teksilo_core::Action::new("doc.count_words")
                        .on_invoke(move |_, _| runs.set(runs.get() + 1)),
                );
                self.child = Some(ctx.add(TextInput::new(Signal::new("hello".to_string()))));
                self.child.into_iter().collect()
            }
            fn layout_response(
                &self,
                proposal: SizeProposal,
                ctx: &teksilo_core::LayoutContext,
            ) -> teksilo_core::LayoutResponse {
                self.child
                    .and_then(|id| ctx.child_size(id, proposal))
                    .unwrap_or_default()
                    .into()
            }
            fn children(&self) -> Vec<WidgetId> {
                self.child.into_iter().collect()
            }
        }

        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        tree.shortcut_registry_mut().register(
            Shortcut::new("doc.count_words")
                .name("Count words")
                .category("Document")
                .build(),
        );
        let runs = Rc::new(Cell::new(0));
        let document = tree.add(Document {
            runs: runs.clone(),
            child: None,
        });
        tree.layout(SizeProposal::exact(800.0, 600.0));
        let field = tree.first_focusable_descendant(document).unwrap();
        tree.focus(field);

        tree.run_with_event_context(&mut teksilo_core::window::NoopWindowOps, |ctx| {
            CommandPalette::new().present(ctx)
        });
        for queued in tree.drain_pending_modal_requests() {
            present_in_tree_modal_request(&mut tree, queued.source_widget, queued.request);
        }
        tree.layout(SizeProposal::exact(800.0, 600.0));
        assert_ne!(tree.focused(), Some(field), "the palette took focus");

        tree.type_text(field, "count words");
        tree.layout(SizeProposal::exact(800.0, 600.0));
        tree.press_key(Key::Enter, Default::default());
        tree.layout(SizeProposal::exact(800.0, 600.0));
        assert_eq!(tree.focused(), Some(field), "closing hands focus back");
        assert_eq!(runs.get(), 1, "the action around the field ran the command");
    }

    /// **A palette opened with nothing focused runs its command from the
    /// window's root.**
    ///
    /// Closing it gives no focus back, so the intent starts where one with no
    /// source does, and reaches the root's own action and the window's global
    /// one: the palette, closed, is no longer a barrier. Before, it started at
    /// the palette's row and stopped at the closed palette.
    #[test]
    fn a_palette_command_with_nothing_focused_runs_from_the_root() {
        use std::cell::RefCell;
        use std::rc::Rc;
        use teksilo_core::event::Key;
        use teksilo_core::shortcut::Shortcut;
        use teksilo_widgets::CommandPalette;

        type Heard = Rc<RefCell<Vec<&'static str>>>;
        /// Answers `doc.save` itself and `doc.export` window-wide.
        #[derive(Debug)]
        struct Page(Heard);
        impl teksilo_core::widget::Widget for Page {
            fn build(&mut self, ctx: &mut teksilo_core::BuildContext) -> Vec<WidgetId> {
                let heard = self.0.clone();
                ctx.register_action(
                    teksilo_core::Action::new("doc.save")
                        .on_invoke(move |_, _| heard.borrow_mut().push("save")),
                );
                let heard = self.0.clone();
                ctx.register_action_global(
                    teksilo_core::Action::new("doc.export")
                        .on_invoke(move |_, _| heard.borrow_mut().push("export")),
                );
                Vec::new()
            }
            fn layout_response(
                &self,
                proposal: SizeProposal,
                _ctx: &teksilo_core::LayoutContext,
            ) -> teksilo_core::LayoutResponse {
                proposal.resolve(800.0, 600.0).into()
            }
        }

        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        for (id, name) in [
            ("doc.save", "Save document"),
            ("doc.export", "Export document"),
        ] {
            tree.shortcut_registry_mut()
                .register(Shortcut::new(id).name(name).category("Document").build());
        }
        let heard = Heard::default();
        tree.add(Page(heard.clone()));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        assert_eq!(tree.focused(), None);

        for query in ["save document", "export document"] {
            tree.run_with_event_context(&mut teksilo_core::window::NoopWindowOps, |ctx| {
                CommandPalette::new().present(ctx)
            });
            for queued in tree.drain_pending_modal_requests() {
                present_in_tree_modal_request(&mut tree, queued.source_widget, queued.request);
            }
            tree.layout(SizeProposal::exact(800.0, 600.0));
            let field = tree.focused().expect("the palette took focus");
            tree.type_text(field, query);
            tree.layout(SizeProposal::exact(800.0, 600.0));
            tree.press_key(Key::Enter, Default::default());
            tree.layout(SizeProposal::exact(800.0, 600.0));
            assert!(tree.active_overlays().is_empty(), "the palette closed");
        }
        assert_eq!(*heard.borrow(), ["save", "export"]);
    }

    /// **The palette's own chord closes it again from inside.**
    ///
    /// The palette is modal, so the page's shortcut that opened it does not
    /// apply with focus in it; the palette answers that chord itself (and
    /// the intent, for a shortcut registered with no owner, which still
    /// fires there), as a toggle the user expects to press twice.
    #[test]
    fn the_palette_toggle_closes_it_from_inside() {
        use teksilo_core::event::{Key, Modifiers};
        use teksilo_core::shortcut::{KeyStroke, Shortcut};
        use teksilo_widgets::CommandPalette;

        /// Opens the palette on `app.palette`, registering its chord itself
        /// when `owned`.
        #[derive(Debug)]
        struct Page {
            owned: bool,
        }
        impl teksilo_core::widget::Widget for Page {
            fn build(&mut self, ctx: &mut teksilo_core::BuildContext) -> Vec<WidgetId> {
                if self.owned {
                    ctx.register_shortcut_global(
                        Shortcut::new("app.palette")
                            .primary(KeyStroke::new(
                                Key::P,
                                Modifiers::COMMAND | Modifiers::SHIFT,
                            ))
                            .build(),
                    );
                }
                ctx.register_action(
                    teksilo_core::Action::new("app.palette")
                        .on_invoke(|_, ctx| CommandPalette::new().present(ctx)),
                );
                Vec::new()
            }
            fn layout_response(
                &self,
                proposal: SizeProposal,
                _ctx: &teksilo_core::LayoutContext,
            ) -> teksilo_core::LayoutResponse {
                proposal.resolve(800.0, 600.0).into()
            }
        }

        for owned in [true, false] {
            let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
            if !owned {
                tree.shortcut_registry_mut().register(
                    Shortcut::new("app.palette")
                        .primary(KeyStroke::new(
                            Key::P,
                            Modifiers::COMMAND | Modifiers::SHIFT,
                        ))
                        .build(),
                );
            }
            tree.add(Page { owned });
            tree.layout(SizeProposal::exact(800.0, 600.0));

            tree.press_key(Key::P, Modifiers::COMMAND | Modifiers::SHIFT);
            for queued in tree.drain_pending_modal_requests() {
                present_in_tree_modal_request(&mut tree, queued.source_widget, queued.request);
            }
            tree.layout(SizeProposal::exact(800.0, 600.0));
            assert!(
                !tree.active_overlays().is_empty(),
                "owned {owned}: it opened"
            );

            tree.press_key(Key::P, Modifiers::COMMAND | Modifiers::SHIFT);
            tree.layout(SizeProposal::exact(800.0, 600.0));
            assert!(
                tree.drain_pending_modal_requests().is_empty(),
                "owned {owned}: no second palette"
            );
            assert!(
                tree.active_overlays().is_empty(),
                "owned {owned}: the same chord closed it"
            );
        }
    }

    /// **A modal whose content is a text editor opens with the caret in it.**
    ///
    /// The editors are the one focusable widget family that carries no label,
    /// so `first_focusable_descendant` is the only thing that can find them —
    /// and a modal that fails to focus one opens with no caret at all, which
    /// reads as a broken surface rather than an unfocused one.
    #[test]
    fn present_in_tree_modal_focuses_a_rich_text_editor() {
        use teksilo_widgets::rich_text::RichTextEditor;
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        tree.focus(source);

        let doc = teksilo_text::text_document::TextDocument::new();
        doc.set_plain_text("hello").unwrap();
        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(move |tree| {
                tree.add(ModalContainer::new(RichTextEditor::editor(doc)))
            })
            .presentation(ModalPresentation::InTree),
        );

        let focused = tree.focused().expect("the modal moved focus into itself");
        assert_ne!(
            focused, source,
            "focus must leave the trigger and land inside the modal"
        );
        let name = tree.widget_type_name(focused).unwrap_or("<none>");
        assert!(
            name.contains("RichTextEditor"),
            "focus landed on {name}, not the editor — the modal opens caretless"
        );
    }

    /// **A modal that opens over a text editor shows its caret.**
    ///
    /// Focus landing on the editor is not enough: the caret is gated on the
    /// editor's *own* `has_focus`, and `present_in_tree_modal_request` parks
    /// the content dormant and re-activates it in the same batch, before
    /// moving focus in. Those two activation edges used to be replayed in
    /// order *after* the focus dispatch, so the superseded `false` arrived
    /// last and the editor's dormancy handler wiped the focus it had just
    /// been granted — the dialog opened with the text visible and no caret,
    /// which reads as a dead surface rather than an unfocused one.
    ///
    /// Asserted on the painted frame rather than on any internal flag,
    /// because the caret is the whole point: a thin, full-line-height rect
    /// in the theme's `editor_caret` colour, at the editor's origin.
    #[test]
    fn present_in_tree_modal_paints_the_editor_caret() {
        use teksilo_widgets::rich_text::RichTextEditor;
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        tree.focus(source);

        let doc = teksilo_text::text_document::TextDocument::new();
        doc.set_plain_text("hello").unwrap();
        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(move |tree| {
                tree.add(ModalContainer::new(RichTextEditor::editor(doc)))
            })
            .presentation(ModalPresentation::InTree),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let editor = tree.focused().expect("the modal moved focus into itself");
        let editor_bounds = tree.bounds(editor);
        let frame = tree.render();

        // The caret is emitted through `Canvas::fill_rect`, which lands in the
        // frame as a `WidgetBackground` decoration — so identify it by shape
        // and colour rather than by kind.
        let caret_color = teksilo_core::presets::intui::light()
            .colors
            .editor_caret
            .to_array();
        let caret = frame.decorations.iter().find(|d| {
            d.color == caret_color && d.rect[2] > 0.0 && d.rect[2] <= 4.0 && d.rect[3] > 4.0
        });
        let caret = caret.unwrap_or_else(|| {
            panic!(
                "the modal painted no caret — {} glyphs and {} decorations, none caret-shaped: {:?}",
                frame.glyphs.len(),
                frame.decorations.len(),
                frame.decorations,
            )
        });

        // ...and it sits inside the editor, not stranded at the viewport origin.
        assert!(
            caret.rect[0] >= editor_bounds.x
                && caret.rect[0] <= editor_bounds.x + editor_bounds.width
                && caret.rect[1] >= editor_bounds.y
                && caret.rect[1] <= editor_bounds.y + editor_bounds.height,
            "caret at {:?} must fall inside the editor's bounds {editor_bounds:?}",
            caret.rect,
        );
    }

    /// Same, but with the editor buried under the chrome a real dialog wraps it
    /// in — a titled panel, a column, a fixed-size box, padding. The walk has to
    /// reach through all of it.
    #[test]
    fn present_in_tree_modal_focuses_an_editor_under_chrome() {
        use teksilo_widgets::rich_text::RichTextEditor;
        use teksilo_widgets::{Divider, FixedSize, Padding, Panel, TextWidget, VStack};
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        tree.layout(SizeProposal::exact(900.0, 700.0));
        tree.focus(source);

        let doc = teksilo_text::text_document::TextDocument::new();
        doc.set_plain_text("hello").unwrap();
        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(move |tree| {
                tree.add(ModalContainer::new(
                    Panel::new().corner_radius(10.0).padding(0.0).child(
                        VStack::new()
                            .spacing(0.0)
                            .child(
                                Padding::symmetric(8.0, 14.0)
                                    .child(TextWidget::new(lit!("Synopsis"))),
                            )
                            .child(Divider::new())
                            .child(
                                FixedSize::new().width(600.0).height(400.0).child(
                                    Padding::uniform(16.0).child(RichTextEditor::editor(doc)),
                                ),
                            ),
                    ),
                ))
            })
            .presentation(ModalPresentation::InTree),
        );

        let focused = tree.focused().expect("the modal moved focus into itself");
        let name = tree.widget_type_name(focused).unwrap_or("<none>");
        assert!(
            name.contains("RichTextEditor"),
            "focus landed on {name}, not the editor — chrome between the modal root \
             and the editor is hiding it from the focus walk"
        );
    }

    /// And with **no `ModalContainer`** — the shape an app takes when its dialog
    /// owns its own chrome (Skribisto's synopsis / picker panels do). The focus
    /// walk starts at whatever the deferred builder returned.
    #[test]
    fn present_in_tree_modal_focuses_an_editor_without_a_modal_container() {
        use teksilo_widgets::rich_text::RichTextEditor;
        use teksilo_widgets::{FixedSize, Padding, Panel, VStack};
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        tree.layout(SizeProposal::exact(900.0, 700.0));
        tree.focus(source);

        let doc = teksilo_text::text_document::TextDocument::new();
        doc.set_plain_text("hello").unwrap();
        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(move |tree| {
                tree.add(
                    Panel::new().corner_radius(10.0).padding(0.0).child(
                        VStack::new().spacing(0.0).child(
                            FixedSize::new()
                                .width(600.0)
                                .height(400.0)
                                .child(Padding::uniform(16.0).child(RichTextEditor::editor(doc))),
                        ),
                    ),
                )
            })
            .presentation(ModalPresentation::InTree),
        );

        let focused = tree.focused().expect("the modal moved focus into itself");
        let name = tree.widget_type_name(focused).unwrap_or("<none>");
        assert!(
            name.contains("RichTextEditor"),
            "focus landed on {name}, not the editor"
        );
    }

    #[test]
    fn present_in_tree_modal_restores_focus_to_trigger_on_dismiss() {
        // Regression: tabbing to a trigger, opening a modal, then
        // dismissing it must return keyboard focus to the trigger. The
        // modal overlay carries the pre-modal focus owner as its
        // `focus_restore`, which every dismiss path replays.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Rename")));
        tree.layout(SizeProposal::exact(800.0, 600.0));
        tree.focus(source);
        assert_eq!(
            tree.focused(),
            Some(source),
            "precondition: trigger focused"
        );

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(|tree| {
                tree.add(ModalContainer::new(Button::new(lit!("Continue"))))
            })
            .presentation(ModalPresentation::InTree),
        );

        // Focus moved into the modal (existing behavior).
        let continue_button = tree.find_by_label("Continue").unwrap();
        assert_eq!(tree.focused(), Some(continue_button));

        // The modal overlay is the topmost; dismissing it must surface
        // the trigger as the focus_restore target.
        let modal_overlay = *tree
            .active_overlays()
            .last()
            .expect("modal overlay registered");
        let (_dismissed, focus_restore) = tree
            .overlay_manager_mut()
            .dismiss_with_focus_restore(modal_overlay);
        assert_eq!(
            focus_restore,
            Some(source),
            "dismissing the modal must restore focus to the trigger that opened it",
        );
    }

    #[test]
    fn mouse_opened_modal_restores_pointer_modality_on_dismiss() {
        // Regression: a modal opened by mouse (focus_visible = false) must
        // not leave the trigger sporting a keyboard `:focus-visible` ring
        // after the user types / presses Enter inside the dialog — which
        // flips the global modality to keyboard. The pre-modal modality is
        // captured and replayed when the overlay dismisses.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Rename")));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        // Mouse-style entry: pointer modality, trigger focused.
        let focus_visible = tree.focus_visible_signal();
        focus_visible.set(false);
        tree.focus(source);

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(|tree| {
                tree.add(ModalContainer::new(Button::new(lit!("Continue"))))
            })
            .presentation(ModalPresentation::InTree),
        );

        // Keyboard input *inside* the dialog (typing the name, Enter to
        // accept) flips the global modality to keyboard.
        focus_visible.set(true);

        // Dismiss fires the overlay's on_dismiss, which restores modality.
        let modal_overlay = *tree
            .active_overlays()
            .last()
            .expect("modal overlay registered");
        // Through the tree, not `overlay_manager_mut()`: the manager parks a
        // dismissal callback rather than running it (it takes an
        // `EventContext` and the manager has no tree), and `dismiss_overlay`
        // is the door that drains it.
        tree.dismiss_overlay(modal_overlay);

        assert!(
            !focus_visible.get(),
            "a mouse-opened modal must restore pointer modality on dismiss, \
             not leave a keyboard focus ring on the trigger",
        );
    }

    #[test]
    fn keyboard_opened_modal_keeps_focus_visible_on_dismiss() {
        // Invariant guard for the fix above: a modal opened while in
        // keyboard modality must KEEP the focus ring on the trigger when it
        // closes — restoring the captured modality must not blanket-clear it.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Rename")));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        // Keyboard-style entry: keyboard modality, trigger focused.
        let focus_visible = tree.focus_visible_signal();
        focus_visible.set(true);
        tree.focus(source);

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(|tree| {
                tree.add(ModalContainer::new(Button::new(lit!("Continue"))))
            })
            .presentation(ModalPresentation::InTree),
        );

        // Even if a pointer event flipped modality off inside the dialog,
        // dismiss restores the captured (keyboard) modality.
        focus_visible.set(false);

        let modal_overlay = *tree
            .active_overlays()
            .last()
            .expect("modal overlay registered");
        // Through the tree, not `overlay_manager_mut()`: the manager parks a
        // dismissal callback rather than running it (it takes an
        // `EventContext` and the manager has no tree), and `dismiss_overlay`
        // is the door that drains it.
        tree.dismiss_overlay(modal_overlay);

        assert!(
            focus_visible.get(),
            "a keyboard-opened modal must restore keyboard modality on dismiss",
        );
    }

    /// Test content widget: a focusable container with two focusable
    /// button descendants. `hint` controls which (if any) the widget
    /// reports as its `initial_focus_hint`.
    #[derive(Debug)]
    struct TwoButtonContent {
        root: Option<WidgetId>,
        second: Option<WidgetId>,
        hint_to_second: bool,
    }

    impl teksilo_core::Widget for TwoButtonContent {
        fn build(&mut self, ctx: &mut teksilo_core::BuildContext) -> Vec<WidgetId> {
            let first = ctx.add(Button::new(lit!("First")));
            let second = ctx.add(Button::new(lit!("Second")));
            let row = ctx.add(teksilo_widgets::HStack::new().child(first).child(second));
            self.root = Some(row);
            self.second = Some(second);
            vec![row]
        }

        fn layout_response(
            &self,
            proposal: teksilo_canvas::SizeProposal,
            ctx: &teksilo_core::LayoutContext,
        ) -> teksilo_core::widget::LayoutResponse {
            self.root
                .and_then(|id| ctx.child_size(id, proposal))
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
                .into()
        }

        fn initial_focus_hint(&self) -> Option<WidgetId> {
            if self.hint_to_second {
                self.second
            } else {
                None
            }
        }

        fn children(&self) -> Vec<WidgetId> {
            self.root.into_iter().collect()
        }
    }

    #[test]
    fn present_in_tree_modal_consults_initial_focus_hint() {
        // When `focus_target` is None, the framework must consult the
        // content widget's `initial_focus_hint` before falling back to
        // `first_focusable_descendant`.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(|tree| {
                tree.add(TwoButtonContent {
                    root: None,
                    second: None,
                    hint_to_second: true,
                })
            })
            .presentation(ModalPresentation::InTree),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        // Two "Second" labels may exist globally (source isn't one), so
        // find_by_label is unambiguous here.
        let second = tree.find_by_label("Second").unwrap();
        assert_eq!(
            tree.focused(),
            Some(second),
            "initial_focus_hint must redirect focus away from first focusable",
        );
    }

    #[test]
    fn present_in_tree_modal_falls_back_to_first_focusable_without_hint() {
        // Baseline: content without an initial_focus_hint gets the first
        // focusable descendant, matching prior behavior.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(|tree| {
                tree.add(TwoButtonContent {
                    root: None,
                    second: None,
                    hint_to_second: false,
                })
            })
            .presentation(ModalPresentation::InTree),
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let first = tree.find_by_label("First").unwrap();
        assert_eq!(
            tree.focused(),
            Some(first),
            "without focus_target or initial_focus_hint, first focusable wins",
        );
    }

    #[test]
    fn present_in_tree_modal_rejects_focus_target_outside_content_subtree() {
        // A focus_target pointing at a widget that exists but is NOT a
        // descendant of content_id must be rejected. The framework falls
        // back to initial_focus_hint → first_focusable_descendant.
        let mut tree = WidgetTree::new().with_theme(teksilo_core::presets::intui::light());
        let source = tree.add(Button::new(lit!("Trigger")));
        tree.layout(SizeProposal::exact(800.0, 600.0));

        present_in_tree_modal_request(
            &mut tree,
            source,
            ModalRequest::deferred(|tree| {
                tree.add(TwoButtonContent {
                    root: None,
                    second: None,
                    hint_to_second: false,
                })
            })
            .presentation(ModalPresentation::InTree)
            .focus_target(source), // active but outside modal subtree
        );
        tree.layout(SizeProposal::exact(800.0, 600.0));

        let first = tree.find_by_label("First").unwrap();
        assert_eq!(
            tree.focused(),
            Some(first),
            "focus_target outside content subtree must be rejected",
        );
    }
}

#[cfg(test)]
mod occluded_band_tests {
    use super::occluded_band_in_client;

    /// A keyboard across the bottom of the screen, under a window that starts
    /// 100 physical pixels down. The band reported to the tree is in the
    /// window's own logical coordinates, clipped to its client area.
    #[test]
    fn a_keyboard_band_is_clipped_and_delogicalised() {
        let band = occluded_band_in_client(
            (0, 100),
            (800, 600),
            2.0,
            // Screen-space: the bottom 400 physical pixels, wider than the
            // window on both sides.
            (-50, 400, 1000, 900),
        )
        .expect("the band covers the window");
        // x clips to 0..800 physical → 0..400 logical; y is 300..600 physical
        // (400-100 .. clipped at 600) → 150..300 logical.
        assert_eq!(band.x, 0.0);
        assert_eq!(band.width, 400.0);
        assert_eq!(band.y, 150.0);
        assert_eq!(band.height, 150.0);
    }

    #[test]
    fn a_keyboard_that_misses_the_window_reports_nothing() {
        // Below a window that ends at y = 100 + 600.
        assert!(occluded_band_in_client((0, 100), (800, 600), 1.0, (0, 800, 800, 1000)).is_none());
        // Beside it.
        assert!(occluded_band_in_client((0, 0), (800, 600), 1.0, (900, 0, 1200, 600)).is_none());
    }

    #[test]
    fn a_degenerate_scale_reports_nothing_rather_than_dividing_by_it() {
        assert!(occluded_band_in_client((0, 0), (800, 600), 0.0, (0, 0, 800, 600)).is_none());
    }
}

/// Whether a winit event-loop failure means the connection to the display
/// server was lost.
///
/// winit has no variant that says so. Both Linux backends turn a failed
/// dispatch or flush into `ExitFailure(errno)` and unwind, deliberately:
/// "crashing downstream is not really an option", as their own comment puts
/// it. Nothing in teksilo ever asks the loop to exit with a non-zero code
/// (`ActiveEventLoop::exit` sets zero), so a non-zero `ExitFailure` is that
/// and nothing else.
///
/// The remaining variants stay fatal. `NotSupported`, `Os` and
/// `RecreationAttempt` are genuine faults at startup, where a panic and its
/// backtrace are the useful answer.
fn display_connection_lost(err: &winit::error::EventLoopError) -> bool {
    matches!(err, winit::error::EventLoopError::ExitFailure(code) if *code != 0)
}

#[cfg(test)]
mod quit_policy_tests {
    use super::{AppEvent, QuitPolicy, TeksiloAppBuilder, WindowConfig};

    #[test]
    fn the_default_quits_with_the_last_window_and_not_before() {
        let policy = QuitPolicy::default();
        assert_eq!(policy, QuitPolicy::LastWindowClosed);
        assert!(policy.exits_with(0, false));
        assert!(!policy.exits_with(1, false));
        assert!(!policy.exits_with(3, false));
    }

    #[test]
    fn explicit_never_quits_for_want_of_a_window() {
        for open in [0, 1, 3] {
            assert!(!QuitPolicy::Explicit.exits_with(open, false));
        }
    }

    /// A completed quit ends the loop under every policy.
    #[test]
    fn a_completed_quit_ends_the_loop_under_every_policy() {
        for policy in [QuitPolicy::LastWindowClosed, QuitPolicy::Explicit] {
            assert!(policy.exits_with(0, true));
        }
    }

    /// With no initial window, the default policy still reaches the loop: an
    /// application root may open the first window from its mount actions,
    /// which run at startup. The check waits for them (see
    /// `startup_without_a_window_is_judged_by_the_windows_left`).
    #[test]
    #[should_panic(expected = "reached the event loop")]
    fn no_initial_window_under_the_default_policy_reaches_the_loop() {
        TeksiloAppBuilder::new().run_with(
            || -> winit::event_loop::EventLoop<AppEvent> { panic!("reached the event loop") },
            |_, _| unreachable!(),
        );
    }

    /// After startup (the initial window, then the application tree's mount
    /// actions), the default policy refuses to go on with no window open:
    /// it would quit at once, a silent exit with status zero. `Explicit`
    /// lives with none.
    #[test]
    fn startup_without_a_window_is_judged_by_the_windows_left() {
        let refusal = QuitPolicy::LastWindowClosed
            .refuses_startup_without_window(0)
            .expect("no window after startup");
        assert!(refusal.contains("quit_policy(QuitPolicy::Explicit)"));
        assert!(refusal.contains("run_after_mount"));
        assert_eq!(
            QuitPolicy::LastWindowClosed.refuses_startup_without_window(1),
            None
        );
        assert_eq!(QuitPolicy::Explicit.refuses_startup_without_window(0), None);
    }

    /// The same builder under `Explicit` gets past the check: it reaches the
    /// loop, which this test replaces with a panic of its own.
    #[test]
    #[should_panic(expected = "reached the event loop")]
    fn no_initial_window_under_explicit_reaches_the_loop() {
        TeksiloAppBuilder::new()
            .quit_policy(QuitPolicy::Explicit)
            .run_with(
                || -> winit::event_loop::EventLoop<AppEvent> { panic!("reached the event loop") },
                |_, _| unreachable!(),
            );
    }

    /// An initial window satisfies the default policy: also past the check.
    #[test]
    #[should_panic(expected = "reached the event loop")]
    fn an_initial_window_satisfies_the_default_policy() {
        TeksiloAppBuilder::new()
            .initial_window(WindowConfig::new())
            .run_with(
                || -> winit::event_loop::EventLoop<AppEvent> { panic!("reached the event loop") },
                |_, _| unreachable!(),
            );
    }
}

#[cfg(test)]
mod app_root_tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
    use teksilo_core::{Action, BuildContext, Intent, NoopWindowOps, WidgetId};

    use super::TeksiloAppBuilder;

    /// Registers `app.ping`, which counts its calls and checks it runs in
    /// no window.
    #[derive(Debug)]
    struct PingRoot(Rc<Cell<u32>>);

    impl Widget for PingRoot {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let pings = self.0.clone();
            ctx.register_action_global(Action::new("app.ping").on_invoke(move |_, ctx| {
                assert!(
                    ctx.window().is_none(),
                    "the application tree is no window's"
                );
                pings.set(pings.get() + 1);
            }));
            Vec::new()
        }

        fn layout_response(
            &self,
            proposal: teksilo_canvas::SizeProposal,
            _ctx: &LayoutContext,
        ) -> LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }
    }

    #[test]
    fn a_headless_app_builds_the_application_tree_it_was_given() {
        let pings = Rc::new(Cell::new(0));
        let root = pings.clone();
        let mut app = TeksiloAppBuilder::new()
            .app_root(move |tree| tree.add(PingRoot(root)))
            .build_headless();
        let app_tree = app.app_tree.as_mut().expect("an app root was given");
        assert_eq!(app_tree.roots().len(), 1);
        assert!(app_tree.window_state().is_none());
        app_tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
            ctx.send_intent(Intent::new("app.ping"));
        });
        assert_eq!(pings.get(), 1);
    }

    /// The logic frame both kinds of tree run: idle callbacks, then the
    /// layout that runs the bindings.
    #[test]
    fn the_logic_frame_runs_idle_callbacks_and_rebuilds_bound_roots() {
        use teksilo_core::binding::BindingLevel;
        use teksilo_core::signal::Signal;

        #[derive(Debug)]
        struct Counted(Signal<u32>, Rc<Cell<u32>>);
        impl Widget for Counted {
            fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
                self.0
                    .bind_to(ctx.self_id(), ctx.binding_registry(), BindingLevel::Rebuild);
                self.1.set(self.1.get() + 1);
                Vec::new()
            }
            fn layout_response(
                &self,
                proposal: teksilo_canvas::SizeProposal,
                _ctx: &LayoutContext,
            ) -> LayoutResponse {
                proposal.resolve(0.0, 0.0).into()
            }
        }

        let generation = Signal::new(0);
        let builds = Rc::new(Cell::new(0));
        let mut tree = teksilo_core::WidgetTree::new();
        tree.add(Counted(generation.clone(), builds.clone()));
        let idle_ran = Rc::new(Cell::new(false));
        let flag = idle_ran.clone();
        tree.run_with_event_context(&mut NoopWindowOps, move |ctx| {
            ctx.request_idle_callback(move |_| flag.set(true));
        });
        let zero = teksilo_canvas::SizeProposal::exact(0.0, 0.0);
        let budget = std::time::Duration::from_millis(4);
        assert!(super::run_tree_logic_frame(
            &mut tree,
            budget,
            zero,
            &mut NoopWindowOps
        ));
        assert!(idle_ran.get());
        let before = builds.get();
        generation.set(1);
        assert!(!super::run_tree_logic_frame(
            &mut tree,
            budget,
            zero,
            &mut NoopWindowOps
        ));
        assert_eq!(builds.get(), before + 1, "the binding rebuilt the root");
    }

    /// A mount action the application root enqueues in `build` runs again
    /// each time that root is rebuilt: what the multi-window guide's
    /// `app_root` example relies on, which opens the first window from one
    /// and guards it so a rebuild cannot open a second.
    #[test]
    fn an_app_roots_mount_action_runs_again_when_the_root_rebuilds() {
        use teksilo_core::binding::BindingLevel;
        use teksilo_core::signal::Signal;

        #[derive(Debug)]
        struct OpensFirstWindow {
            generation: Signal<u32>,
            mounts: Rc<Cell<u32>>,
            opened_first_window: Rc<Cell<bool>>,
            opens: Rc<Cell<u32>>,
        }
        impl Widget for OpensFirstWindow {
            fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
                self.generation.bind_to(
                    ctx.self_id(),
                    ctx.binding_registry(),
                    BindingLevel::Rebuild,
                );
                let mounts = self.mounts.clone();
                ctx.run_after_mount(move |_| mounts.set(mounts.get() + 1));
                // The guide's guard.
                let (opened, opens) = (self.opened_first_window.clone(), self.opens.clone());
                ctx.run_after_mount(move |_| {
                    if !opened.replace(true) {
                        opens.set(opens.get() + 1);
                    }
                });
                Vec::new()
            }
            fn layout_response(
                &self,
                proposal: teksilo_canvas::SizeProposal,
                _ctx: &LayoutContext,
            ) -> LayoutResponse {
                proposal.resolve(0.0, 0.0).into()
            }
        }

        let generation = Signal::new(0);
        let (mounts, opens) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        let root = OpensFirstWindow {
            generation: generation.clone(),
            mounts: mounts.clone(),
            opened_first_window: Rc::default(),
            opens: opens.clone(),
        };
        let mut app = TeksiloAppBuilder::new()
            .app_root(move |tree| tree.add(root))
            .build_headless();
        let app_tree = app.app_tree.as_mut().expect("an app root was given");
        let zero = teksilo_canvas::SizeProposal::exact(0.0, 0.0);
        let budget = std::time::Duration::from_millis(4);
        super::run_tree_logic_frame(app_tree, budget, zero, &mut NoopWindowOps);
        app_tree.run_mount_actions(&mut NoopWindowOps);
        assert_eq!((mounts.get(), opens.get()), (1, 1), "at startup");
        generation.set(1);
        super::run_tree_logic_frame(app_tree, budget, zero, &mut NoopWindowOps);
        app_tree.run_mount_actions(&mut NoopWindowOps);
        assert_eq!(mounts.get(), 2, "the rebuilt root's mount action ran again");
        assert_eq!(opens.get(), 1, "the guarded one opened nothing more");
    }

    /// However many wakes arrive before the loop takes the first, one is
    /// posted; once taken, the next posts again.
    #[test]
    fn the_app_tree_waker_posts_once_until_taken() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use teksilo_canvas::wake::{RedrawWaker, WakeKind};

        let posts = Arc::new(AtomicUsize::new(0));
        let pending = Arc::new(AtomicBool::new(false));
        let counted = posts.clone();
        let waker = super::AppTreeWaker {
            post: Arc::new(move |payload| {
                assert!(payload.is::<super::AppTreeWake>());
                counted.fetch_add(1, Ordering::SeqCst);
            }),
            pending: pending.clone(),
        };
        waker.wake(WakeKind::Layout);
        waker.wake(WakeKind::Draw);
        waker.wake(WakeKind::Layout);
        assert_eq!(posts.load(Ordering::SeqCst), 1);
        assert!(pending.swap(false, Ordering::SeqCst), "the loop takes it");
        waker.wake(WakeKind::Draw);
        assert_eq!(posts.load(Ordering::SeqCst), 2);
    }

    /// A window's intent that only the application root implements reaches
    /// it in a headless app too, once routed.
    #[test]
    fn a_headless_app_routes_a_windows_intent_to_its_app_root() {
        let pings = Rc::new(Cell::new(0));
        let root = pings.clone();
        let mut app = TeksiloAppBuilder::new()
            .app_root(move |tree| tree.add(PingRoot(root)))
            .build_headless();
        app.tree.add(super::AppTreeAnchor);
        app.tree.run_with_event_context(&mut NoopWindowOps, |ctx| {
            ctx.send_intent(Intent::new("app.ping"));
            ctx.send_intent(Intent::new("app.unknown"));
        });
        assert_eq!(
            app.route_unclaimed_intents(),
            ["app.unknown"],
            "taken by nobody"
        );
        assert_eq!(pings.get(), 1, "app.ping crossed to the app root");
    }

    /// A window root binding Ctrl+N to `app.ping` with a global shortcut,
    /// with no action for it, over a focused field recording its keys.
    #[derive(Debug)]
    struct ShortcutWindow {
        keys: Rc<std::cell::RefCell<Vec<teksilo_core::Key>>>,
        field: Rc<Cell<Option<WidgetId>>>,
    }

    impl Widget for ShortcutWindow {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            use teksilo_core::shortcut::{KeyStroke, Shortcut};
            use teksilo_core::widget_builder::WidgetBuilder;
            use teksilo_core::{EventResponse, Key, WidgetEvent};
            ctx.register_shortcut_global(
                Shortcut::new("app.ping")
                    .primary(KeyStroke::ctrl(Key::N))
                    .build(),
            );
            let keys = self.keys.clone();
            let field = ctx.add(teksilo_widgets::RectWidget::new().focusable(true).on_key(
                move |event, _| {
                    if let WidgetEvent::KeyDown { key, .. } = event {
                        keys.borrow_mut().push(*key);
                    }
                    EventResponse::Handled
                },
            ));
            self.field.set(Some(field));
            vec![field]
        }

        fn layout_response(
            &self,
            proposal: teksilo_canvas::SizeProposal,
            _ctx: &LayoutContext,
        ) -> LayoutResponse {
            proposal.resolve(200.0, 100.0).into()
        }
    }

    /// A shortcut nothing in the window takes runs the application tree's
    /// action, and one nobody takes (`edit.copy`, bound in every tree) still
    /// reaches the focused widget as its key.
    #[test]
    fn a_shortcut_only_the_app_root_takes_runs_there_and_one_nobody_takes_falls_through() {
        use teksilo_core::{Key, Modifiers};
        let pings = Rc::new(Cell::new(0));
        let root = pings.clone();
        let mut app = TeksiloAppBuilder::new()
            .app_root(move |tree| tree.add(PingRoot(root)))
            .build_headless();
        let keys = Rc::new(std::cell::RefCell::new(Vec::new()));
        let field = Rc::new(Cell::new(None));
        app.tree.add(ShortcutWindow {
            keys: keys.clone(),
            field: field.clone(),
        });
        app.tree
            .layout(teksilo_canvas::SizeProposal::exact(200.0, 100.0));
        app.tree.focus(field.get().expect("built"));

        app.tree.press_key(Key::N, Modifiers::COMMAND);
        assert!(app.route_unclaimed_intents().is_empty());
        assert_eq!(pings.get(), 1, "Ctrl+N ran the app root's action");
        assert!(keys.borrow().is_empty(), "and the field never heard it");

        app.tree.press_key(Key::C, Modifiers::COMMAND);
        assert!(app.route_unclaimed_intents().is_empty(), "nothing kept");
        assert_eq!(*keys.borrow(), [Key::C], "Ctrl+C reached the field");
    }

    /// `on_app_event` and the app-event observers see what a
    /// `WithActivationToken` wraps, as `on_external_with_ctx` does.
    #[test]
    fn an_app_event_handler_sees_the_payload_an_activation_token_wraps() {
        struct Payload;
        let wrapped =
            super::AppEvent::External(Box::new(super::WithActivationToken::new("tok", Payload)));
        let (event, token) = super::unwrap_activation_token(wrapped);
        assert_eq!(token.as_deref(), Some("tok"));
        assert!(matches!(event, super::AppEvent::External(payload) if payload.is::<Payload>()));
        let (event, token) =
            super::unwrap_activation_token(super::AppEvent::External(Box::new(Payload)));
        assert_eq!(token, None);
        assert!(matches!(event, super::AppEvent::External(payload) if payload.is::<Payload>()));
    }

    /// A page that saves (`doc.save`, a scoped action, with Ctrl+K as a
    /// global shortcut for it) and an Open button presenting an in-window
    /// dialog over it. The dialog answers `dialog.own` itself and has a
    /// button sending each of `doc.save`, `app.ping` and `dialog.own`.
    #[derive(Debug)]
    struct SavingPage {
        log: Rc<std::cell::RefCell<Vec<&'static str>>>,
    }

    impl Widget for SavingPage {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            use teksilo_core::modal::{ModalPresentation, ModalRequest};
            use teksilo_core::shortcut::{KeyStroke, Shortcut};
            use teksilo_widgets::{Button, ModalContainer};
            let log = self.log.clone();
            ctx.register_action(Action::new("doc.save").on_invoke(move |_, _| {
                log.borrow_mut().push("page:doc.save");
            }));
            ctx.register_shortcut_global(
                Shortcut::new("doc.save")
                    .primary(KeyStroke::ctrl(teksilo_core::Key::K))
                    .build(),
            );
            let log = self.log.clone();
            let open = ctx.add(Button::new(teksilo_i18n::lit!("Open")).on_activate_fn(
                move |ctx| {
                    let log = log.clone();
                    ctx.present_modal(
                        ModalRequest::deferred(move |tree| {
                            tree.add(ModalContainer::new(DialogBody { log }))
                        })
                        .presentation(ModalPresentation::InTree),
                    );
                },
            ));
            vec![open]
        }

        fn layout_response(
            &self,
            proposal: teksilo_canvas::SizeProposal,
            _ctx: &LayoutContext,
        ) -> LayoutResponse {
            proposal.resolve(800.0, 600.0).into()
        }
    }

    #[derive(Debug)]
    struct DialogBody {
        log: Rc<std::cell::RefCell<Vec<&'static str>>>,
    }

    impl Widget for DialogBody {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            use teksilo_widgets::{Button, VStack};
            let log = self.log.clone();
            ctx.register_action(Action::new("dialog.own").on_invoke(move |_, _| {
                log.borrow_mut().push("dialog:dialog.own");
            }));
            let mut stack = VStack::new();
            for name in ["doc.save", "app.ping", "dialog.own"] {
                stack = stack.child(
                    Button::new(teksilo_i18n::lit!(name))
                        .on_activate_fn(move |ctx| ctx.send_intent(Intent::new(name))),
                );
            }
            vec![ctx.add(stack)]
        }

        fn layout_response(
            &self,
            proposal: teksilo_canvas::SizeProposal,
            _ctx: &LayoutContext,
        ) -> LayoutResponse {
            proposal.resolve(300.0, 120.0).into()
        }
    }

    /// The in-window counterpart of a native dialog window's leftovers going
    /// to the window it blocks: an intent sent by a button of an in-window
    /// dialog that is still open goes to the dialog's own action first, then
    /// to the page behind it, then to the application tree. A key in the
    /// dialog stops at it: the page's global shortcut does not fire there.
    #[test]
    fn an_open_in_window_dialogs_leftover_intent_reaches_the_page_then_the_application() {
        use teksilo_core::accesskit::Action as AtAction;
        use teksilo_core::{Key, Modifiers, WidgetEvent};
        let pings = Rc::new(Cell::new(0));
        let root = pings.clone();
        let mut app = TeksiloAppBuilder::new()
            .theme(teksilo_core::presets::intui::light())
            .app_root(move |tree| tree.add(PingRoot(root)))
            .build_headless();
        let log = Rc::new(std::cell::RefCell::new(Vec::new()));
        app.tree.add(SavingPage { log: log.clone() });
        let proposal = teksilo_canvas::SizeProposal::exact(800.0, 600.0);
        app.tree.layout(proposal);
        let click = |app: &mut super::HeadlessApp, label: &str| {
            let target = app.tree.find_by_label(label).expect(label);
            app.tree.dispatch_event(WidgetEvent::AccessAction {
                action: AtAction::Click,
                target: Some(target),
                target_node: teksilo_core::accessibility::root_node_id(),
                data: None,
            });
        };
        click(&mut app, "Open");
        for queued in app.tree.drain_pending_modal_requests() {
            super::present_in_tree_modal_request(
                &mut app.tree,
                queued.source_widget,
                queued.request,
            );
        }
        app.tree.layout(proposal);
        assert_eq!(app.tree.active_overlays().len(), 2, "the dialog is up");

        click(&mut app, "dialog.own");
        click(&mut app, "doc.save");
        click(&mut app, "app.ping");
        assert!(app.route_unclaimed_intents().is_empty(), "every one taken");
        assert_eq!(
            *log.borrow(),
            ["dialog:dialog.own", "page:doc.save"],
            "the dialog's own action, then the page's, the dialog open all along"
        );
        assert_eq!(pings.get(), 1, "app.ping reached the application tree");
        assert_eq!(
            app.tree.active_overlays().len(),
            2,
            "the dialog is still up"
        );

        log.borrow_mut().clear();
        app.tree.press_key(Key::K, Modifiers::COMMAND);
        assert!(app.route_unclaimed_intents().is_empty());
        assert!(
            log.borrow().is_empty(),
            "the page's shortcut does not fire in the dialog"
        );
    }

    #[test]
    fn a_headless_app_without_an_app_root_has_no_application_tree() {
        assert!(TeksiloAppBuilder::new().build_headless().app_tree.is_none());
    }
}

#[cfg(test)]
mod display_connection_tests {
    use super::display_connection_lost;
    use winit::error::EventLoopError;

    /// The defect this pins: a compositor that exits under a running
    /// application took the process down through a panic and a crash report,
    /// when the only honest reading is that the session ended.
    #[test]
    fn a_failed_dispatch_reads_as_a_lost_display_server() {
        assert!(display_connection_lost(&EventLoopError::ExitFailure(1)));
        assert!(display_connection_lost(&EventLoopError::ExitFailure(32)));
    }

    /// A clean `exit()` carries code zero, and must not be mistaken for the
    /// compositor leaving.
    #[test]
    fn a_clean_exit_is_not_a_lost_display_server() {
        assert!(!display_connection_lost(&EventLoopError::ExitFailure(0)));
    }

    /// Everything else is a real fault and stays fatal, so that narrowing the
    /// panic does not quietly swallow the failures it was there for.
    #[test]
    fn a_loop_that_could_not_be_built_stays_fatal() {
        assert!(!display_connection_lost(&EventLoopError::RecreationAttempt));
    }
}

#[cfg(all(test, feature = "live-image-timings"))]
mod live_trace_tests {
    use super::*;

    #[test]
    fn the_live_line_prints_its_deltas_and_the_percentiles_in_microseconds() {
        let mut stats = teksilo_render::LiveTextureStats::default();
        (stats.textures, stats.bytes) = (1, 3_686_400);
        (stats.uploads_full, stats.uploads_partial, stats.contended) = (2, 61, 3);
        stats.sibling_copies = 5;
        let mut timings = teksilo_render::LiveImageTimings::default();
        let p = &mut timings.prepare;
        (p.p50, p.p90, p.p99, p.max, p.samples) = (96, 150, 291, 410, 1024);
        let p = &mut timings.lock_hold;
        (p.p50, p.p90, p.p99, p.max, p.samples) = (95, 140, 296, 400, 2048);
        assert_eq!(
            live_trace_line(12.5, "main", &stats, &timings, (3, 4, 1)),
            "teksilo_idle_trace_live t=12.500 window=\"main\" textures=1 bytes=3686400 \
             uploads=60 copies=1 contended=2 \
             prepare_us={p50:96,p90:150,p99:291,max:410,n:1024} \
             lock_hold_us={p50:95,p90:140,p99:296,max:400,n:2048} \
             commit_to_upload_us={p50:0,p90:0,p99:0,max:0,n:0}"
        );
    }

    #[test]
    fn a_report_owes_a_line_to_each_window_seen_rendering_since_the_last() {
        let (a, b) = (WindowId::from(1), WindowId::from(2));
        let mut trace = IdleTrace::new();
        trace.note_rendered_frame(a);
        trace.note_rendered_frame(a);
        assert!(trace.live.due.is_empty(), "no report yet: nothing is owed");
        trace.last_report = Instant::now() - Duration::from_secs(2);
        trace.note_rendered_frame(b);
        trace.live.due.sort_by_key(|id| u64::from(*id));
        assert_eq!(trace.live.due, vec![a, b], "each window once");
        assert!(trace.live.rendered.is_empty());

        // The line is printed, and the debt paid, by `report_live`, which
        // needs a window: the next report owes only what rendered since.
        trace.live.due.clear();
        trace.last_report = Instant::now() - Duration::from_secs(2);
        trace.note_tick();
        assert!(trace.live.due.is_empty(), "a tick renders nothing");
    }
}

#[cfg(test)]
mod a11y_delivery_tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn a_frame_owing_the_adapter_nothing_neither_delivers_nor_asks_for_a_frame() {
        let t0 = Instant::now();
        for walk_changed in [false, true] {
            for since in [None, Some(t0), Some(t0 - 500 * MS)] {
                assert_eq!(
                    a11y_delivery(false, false, walk_changed, since, t0 + 10 * MS),
                    A11yDelivery::Nothing,
                    "walk changed {walk_changed}, last delivery {since:?}"
                );
            }
        }
    }

    #[test]
    fn moves_go_ten_a_second_and_the_rest_at_once() {
        let t0 = Instant::now();
        assert_eq!(
            a11y_delivery(true, false, false, Some(t0), t0 + 40 * MS),
            A11yDelivery::HoldUntil(t0 + MOVE_DELIVERY_INTERVAL),
            "a move within the interval waits for its end"
        );
        assert_eq!(
            a11y_delivery(true, false, false, Some(t0), t0 + 100 * MS),
            A11yDelivery::Deliver
        );
        assert_eq!(
            a11y_delivery(true, false, true, Some(t0), t0 + 40 * MS),
            A11yDelivery::Deliver,
            "a walk is not throttled"
        );
        assert_eq!(
            a11y_delivery(true, false, false, None, t0),
            A11yDelivery::Deliver,
            "nor is the first delivery"
        );
        assert_eq!(
            a11y_delivery(false, true, false, Some(t0), t0 + MS),
            A11yDelivery::Deliver,
            "the adapter asked for the whole tree"
        );
    }
}

/// The capture caveat a screenshot carries for a web view: a page drawn by a
/// native view over the window is missing from it, one drawn into the frame
/// is not.
#[cfg(any(test, all(feature = "automation", debug_assertions)))]
fn web_view_hole_warning(has_web_view: bool, drawn_in_frame: bool) -> Vec<String> {
    if has_web_view && !drawn_in_frame {
        vec!["webview_hole_possible".to_string()]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod web_view_hole_tests {
    use super::web_view_hole_warning;

    #[test]
    fn only_a_native_web_view_leaves_a_hole_in_a_capture() {
        assert_eq!(
            web_view_hole_warning(true, false),
            vec!["webview_hole_possible"]
        );
        assert!(web_view_hole_warning(true, true).is_empty());
        assert!(web_view_hole_warning(false, false).is_empty());
    }
}
