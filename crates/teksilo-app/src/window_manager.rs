// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Multi-window management.
//!
//! The `WindowManager` maintains a collection of active windows, each with its
//! own `WidgetTree`, `PlatformWindow`, and translation state. It routes events
//! by winit `WindowId`, broadcasts environment changes to all windows, and
//! handles modal dialog blocking.

use std::collections::HashMap;
use std::rc::Rc;

use teksilo_core::Theme;
use teksilo_core::event_source::TreeAppContext;
use teksilo_core::signal::Prop;
use teksilo_core::{
    CloseBlockedCallback, CloseGuard, CloseResponse, DecorationsMode, PlatformTitleBarHost,
    TitleBarHostCallbacks, UserAttentionKind, WidgetId, WidgetTree, WindowCommand, WindowPlacement,
    WindowState, WindowStateInit,
};
use teksilo_platform::AccessibilityPreferences;
use teksilo_platform::PlatformWindow;
use teksilo_platform::create_title_bar_host;
use teksilo_platform::event_translation::TranslationState;
#[allow(unused_imports)]
use winit::raw_window_handle::HasWindowHandle;
use winit::window::WindowLevel;

use crate::app::{AppEventProxy, CloseWindowRequest, ThemeMode};
use crate::native_theme::{
    NativeThemeFactory, OsQueries, OsThemeInputs, OsThemeSource, OsThemeUpdate, resolve_os_theme,
};

use crate::window_config::{TeksiloWindowId, WindowConfig};

/// A queued window closure awaiting the next
/// [`process_pending`](WindowManager::process_pending) tick.
#[derive(Debug, Clone, Copy)]
struct PendingClose {
    /// Which window to close.
    id: TeksiloWindowId,
    /// `true` = close unconditionally (the window's close guard is
    /// skipped). `false` = consult the window's close guard first; a
    /// [`CloseResponse::Veto`](teksilo_core::CloseResponse) keeps the
    /// window open.
    force: bool,
}

/// Pure close-verdict logic used by
/// [`WindowManager::evaluate_close_guard`]: what the window's guard says
/// of the close.
///
/// Precedence:
/// 1. [`can_close`](teksilo_core::WindowConfig::can_close) sugar: a
///    `Some(false)` signal vetoes, or, with `on_close_blocked` set,
///    runs it with a fresh
///    [`CloseDecision`](teksilo_core::CloseDecision) and defers on that
///    decision (the only side effect here).
/// 2. the [`on_close_requested`](teksilo_core::WindowConfig::on_close_requested)
///    guard's [`CloseResponse`].
/// 3. no guard configured: close.
///
/// Factored out of `evaluate_close_guard` so the decision can be
/// unit-tested headlessly with a `NoopWindowOps`-backed `EventContext`,
/// without standing up a winit event loop.
fn close_verdict(
    can_close: &Option<Prop<bool>>,
    on_close_blocked: &Option<CloseBlockedCallback>,
    close_guard: &Option<CloseGuard>,
    ctx: &mut teksilo_core::widget::EventContext,
) -> CloseResponse {
    if let Some(may_close) = can_close
        && !may_close.get()
    {
        if let Some(on_blocked) = on_close_blocked {
            let decision = teksilo_core::CloseDecision::new();
            on_blocked(ctx, decision.clone());
            return CloseResponse::Defer(decision);
        }
        return CloseResponse::Veto;
    }
    if let Some(guard) = close_guard {
        return guard(ctx);
    }
    CloseResponse::Close
}

/// The title-bar buttons `config` offers: always close, minimise and
/// maximise as [`WindowConfig::is_minimizable`] and
/// [`WindowConfig::is_maximizable`] say.
fn caption_buttons(config: &WindowConfig) -> winit::window::WindowButtons {
    use winit::window::WindowButtons;
    let mut buttons = WindowButtons::CLOSE;
    buttons.set(WindowButtons::MINIMIZE, config.is_minimizable());
    buttons.set(WindowButtons::MAXIMIZE, config.is_maximizable());
    buttons
}

/// Decide whether a `WindowEvent::Ime` carrying an (already-classified) preedit
/// is a redundant *consecutive* empty preedit that the event loop should drop
/// without dispatching or redrawing, updating the per-window `last_empty` flag
/// in the process. The first empty preedit after any non-empty IME activity is
/// meaningful (it clears an active composition); only the second-and-later
/// consecutive empties are skipped. Extracted from the `WindowEvent::Ime` arm so
/// the state machine is unit-testable without a winit event loop.
pub(crate) fn ime_should_skip_empty_preedit(last_empty: &mut bool, empty_preedit: bool) -> bool {
    let repeat = empty_preedit && *last_empty;
    *last_empty = empty_preedit;
    repeat
}

/// Wrap the root a window's builder made in the window's post-root hook:
/// its own `post_root`, else the app-wide default (none for a modal
/// window). Returns the wrapped root. The tree remembers the unwrapped one
/// as its [content root](WidgetTree::set_content_root), where intents with
/// no source widget start.
fn wrap_root(
    tree: &mut WidgetTree,
    root_id: WidgetId,
    post_root: Option<teksilo_core::window::PostRootBuilder>,
    default_post_root: Option<crate::DefaultPostRoot>,
) -> WidgetId {
    tree.set_content_root(root_id);
    match (post_root, default_post_root) {
        (Some(post_root), _) => post_root(tree, root_id),
        (None, Some(default_post_root)) => (default_post_root.0)(tree, root_id),
        (None, None) => root_id,
    }
}

/// Where keyboard focus lands when a freshly built window is shown, or `None`
/// to leave the window with nothing focused.
///
/// The two window kinds get deliberately different policies:
///
/// * **Modal** — focus something, always: an explicitly requested target, else
///   the root's [`Widget::initial_focus_hint`](teksilo_core::widget::Widget::initial_focus_hint),
///   else the first focusable descendant. A dialog you must Tab into before you
///   can answer it is broken.
/// * **Plain window** — an *explicit* hint only. There is no
///   `first_focusable_descendant` fallback on purpose: auto-focusing the first
///   focusable of every window would drop the caret into whatever search box or
///   text field happens to come first in tree order, changing behavior for every
///   existing app window. A window that wants directed focus opts in by
///   overriding `initial_focus_hint` — e.g. a launcher pointing at its
///   recent-projects list so **Enter opens the highlighted entry with no Tab
///   first**. (Before this, only modals could direct focus at all, so a plain
///   window opened with nothing focused and its first keystroke went nowhere.)
///
/// The hint lookup walks descendants, so it still resolves when the opting-in
/// widget sits deep under the window chrome (title bar, resize frame, post-root
/// wrapper).
///
/// Extracted from `create_window` so the policy is unit-testable without a winit
/// event loop.
/// Whether the macOS Window menu and the Dock icon's menu list a window
/// opened from `config`. A modal dialog is not listed, as AppKit's sheets
/// and panels are not: it comes forward with the window it blocks. Nor is a
/// window kept out of the taskbar (`WindowConfig::skip_taskbar`, a tool
/// palette), which the user does not switch to either.
fn listed_in_window_menu(config: &WindowConfig) -> bool {
    !config.is_modal() && !config.skip_taskbar
}

fn initial_window_focus(
    tree: &WidgetTree,
    root_id: WidgetId,
    is_modal: bool,
    modal_focus_target: Option<WidgetId>,
) -> Option<WidgetId> {
    if is_modal {
        modal_focus_target
            .filter(|id| tree.is_active(*id))
            .or_else(|| tree.widget_initial_focus_hint(root_id))
            .or_else(|| tree.first_focusable_descendant(root_id))
    } else {
        tree.widget_initial_focus_hint(root_id)
    }
}

/// Per-window state managed by the WindowManager.
pub(crate) struct ManagedWindow {
    pub teksilo_id: TeksiloWindowId,
    pub string_id: Option<String>,
    pub tree: WidgetTree,
    /// Reactive per-window state shared with the `WidgetTree` and
    /// accessible from handlers via
    /// [`EventContext::window`](teksilo_core::widget::EventContext::window).
    pub state: WindowState,
    pub platform_window: PlatformWindow,
    pub translation_state: TranslationState,
    pub current_modifiers: winit::keyboard::ModifiersState,
    pub modal: bool,
    pub parent: Option<TeksiloWindowId>,
    /// Size-to-content mode from `WindowConfig::size_to_content`. When not
    /// `Off`, the redraw path measures the content's intrinsic size after
    /// layout and resizes the OS window to fit — the native-modal analogue of
    /// the in-tree overlay's size-to-content (used for dialogs / MessageBox).
    pub size_to_content: teksilo_core::window::SizeToContent,
    /// The accessibility walk generation last delivered to the adapter, and
    /// when that delivery happened.
    ///
    /// Deliveries that carry only moved geometry are throttled: a scrolling
    /// list moves every visible node on every frame, and on Linux each
    /// changed node costs an `object:bounds-changed` signal plus an
    /// O(text) diff per range-supporting node. A semantic change is
    /// delivered on its own frame; moves coalesce to at most ten a second,
    /// with a wake timer so the last one is not left undelivered.
    pub a11y_delivered_walk: u64,
    pub a11y_delivered_at: Option<std::time::Instant>,
    /// Last size (logical px) applied by the size-to-content auto-resize.
    /// Guards against a measure, resize, re-measure oscillation: a resize is
    /// only issued when the freshly-measured target differs from this.
    pub(crate) last_autosize: Option<(u32, u32)>,
    /// The window's size when [`last_autosize`](Self::last_autosize) was
    /// asked for: a size other than either since means the window system
    /// answered with its own (see [`autosize_step`](crate::app::autosize_step)).
    pub(crate) autosize_asked_from: Option<(u32, u32)>,
    /// With size-to-content, the width follows the content too, never
    /// narrower than this: a native dialog window, whose requested width is
    /// the floor. `None` keeps the width.
    pub(crate) autosize_width_floor: Option<u32>,
    /// For a dialog window centred over its owner, the point (logical, in
    /// the desktop's coordinates) it is centred on, until its size to
    /// content settles: each size it grows to is centred there too (see
    /// [`centred_on`]), so a dialog wider than it asked for is not off to
    /// one side. A later change (a details expander) keeps its corner.
    pub(crate) autosize_centre: Option<(f64, f64)>,
    /// Custom-chrome host, if the window opted in via
    /// `WindowConfig::decorations(DecorationsMode::CustomChrome)` and the
    /// platform supports it.
    /// The same `Rc` is also stored on the `WidgetTree` so the root-builder
    /// closure can hand it to a `TitleBar` widget.
    pub title_bar_host: Option<Rc<dyn PlatformTitleBarHost>>,
    /// Tracks `WindowEvent::Focused`. On Linux/X11, Linux/Wayland, and
    /// Windows this also fires on minimize (no separate minimize event
    /// exists in winit 0.30). We assume focused on creation — winit
    /// may not send `Focused(true)` for the initial window on every
    /// platform, and parking animations before the user has even seen
    /// the window would be wrong.
    pub focused: bool,
    /// Tracks `WindowEvent::Occluded`, which winit 0.30 emits on macOS (from
    /// the window's occlusion state) and on X11 (from `VisibilityNotify`), and
    /// never on Wayland or Windows. Combined with `focused` to decide whether
    /// the widget tree's animation scheduler should run:
    /// `active = focused && !occluded`.
    pub occluded: bool,
    /// The window is minimised, as far as teksilo-app knows: from `Resized`
    /// and its placement, from the initial placement, and on X11, which
    /// reports an iconify with no event, from a rate-limited query while
    /// unfocused ([`refresh_minimized`](Self::refresh_minimized)) and, for a
    /// dialog and the window it blocks, from the window-state watch
    /// ([`WindowManager::reread_wm_states`]). Always
    /// `false` on Wayland, where winit cannot tell; there the compositor
    /// withholds a hidden window's redraws, and the window is ticked as one
    /// whose redraw is withheld (see [`crate::redraw_gate`]).
    pub minimized: bool,
    /// When `minimized` was last read from winit while unfocused; `None`
    /// since the window last lost focus.
    pub(crate) minimized_probed_at: Option<std::time::Instant>,
    /// `occluded` was set from asking the platform, not from winit's event,
    /// and this is when it was last asked; `None` otherwise. A macOS window
    /// AppKit never showed (created while the display slept) never gets
    /// `Occluded(true)`: its skipped frame asks instead
    /// ([`PlatformWindow::occluded_now`](teksilo_platform::PlatformWindow::occluded_now)).
    /// The reveal normally arrives as `Occluded(false)`; asking again every
    /// [`OCCLUSION_PROBE_INTERVAL`] keeps the window from staying blank should
    /// it not.
    pub(crate) occlusion_probed_at: Option<std::time::Instant>,
    /// Nothing on screen can be seen at all, the session watch says: on
    /// macOS, the displays asleep, the screen locked, or another session on
    /// the console, none of which AppKit reports as occlusion for a window
    /// already showing. The same for every window; see
    /// [`WindowManager::set_session_away`].
    pub(crate) session_away: bool,
    /// Caps Lock active state, toggled on each `Key::CapsLock` press
    /// (winit 0.30 delivers Caps Lock as a discrete key, not via
    /// `ModifiersState`). Pushed to `state.caps_lock` so password fields
    /// can warn. Starts `false`; the OS lock state at launch is not
    /// observable on winit 0.30, so it can desync if Caps Lock was
    /// already on before the app gained focus.
    pub caps_lock_active: bool,
    /// What the tree's theme was last resolved from while it followed the
    /// OS ([`ThemeMode::FollowSystem`] or [`ThemeMode::Native`]); `None`
    /// once any other theme was set. A re-read of the OS that gives the
    /// same inputs at the tree's density sets no theme.
    pub(crate) os_theme: Option<OsThemeInputs>,
    /// Last OS-IME enablement applied to the winit window (`None` = never
    /// set, forces the first apply). The post-dispatch reconcile compares
    /// the focused node's IME descriptor against this and calls
    /// `set_ime_allowed` only on change — repeated `set_ime_allowed(true)`
    /// can cancel an active composition on some platforms.
    pub ime_allowed: Option<bool>,
    /// Last OS-IME purpose applied to the winit window. Re-applied whenever
    /// it changes while IME is enabled.
    pub ime_purpose: Option<teksilo_core::ImePurpose>,
    /// Whether the previous `WindowEvent::Ime` was an empty `Preedit("")`.
    /// Some Linux IME backends (ibus / fcitx via winit) flood empty preedits
    /// while a field is focused; the first clears any active composition, but
    /// every consecutive repeat is a no-op that would still wake a full
    /// layout+render pass. This lets the event loop skip the repeats entirely
    /// (neither dispatch nor redraw). Reset by any non-empty-preedit IME event.
    pub last_ime_preedit_empty: bool,
    /// RAII handles for the auto-save observers wired to
    /// `state.{size, position, placement}` when a
    /// `WindowStateService` is registered. Dropped when the window
    /// is removed from `WindowManager::windows`.
    pub _persist_handles: Vec<teksilo_core::ObserverHandle>,
    /// Optional close guard taken from
    /// [`WindowConfig::on_close_requested`](teksilo_core::WindowConfig::on_close_requested).
    /// Consulted by [`process_pending`](WindowManager::process_pending)
    /// before a *guarded* close tears this window down; a
    /// [`CloseResponse::Veto`] cancels the close. `None` = no guard, the
    /// window closes immediately. This is strictly per-window — closing
    /// one window never consults another's guard.
    pub close_guard: Option<CloseGuard>,
    /// Optional reactive "may this window close?" signal from
    /// [`WindowConfig::can_close`](teksilo_core::WindowConfig::can_close).
    /// When present and `false`, a guarded close is blocked: vetoed, or
    /// deferred to [`on_close_blocked`](Self::on_close_blocked) when that
    /// is set. Evaluated
    /// before [`close_guard`](Self::close_guard).
    pub can_close: Option<Prop<bool>>,
    /// Optional hook run when [`can_close`](Self::can_close) blocks a
    /// close; it answers the close's `CloseDecision`. From
    /// [`WindowConfig::on_close_blocked`](teksilo_core::WindowConfig::on_close_blocked).
    pub on_close_blocked: Option<CloseBlockedCallback>,
    /// Optional teardown hook from
    /// [`WindowConfig::on_removed`](teksilo_core::WindowConfig::on_removed).
    /// Invoked once by [`close_window`](WindowManager::close_window),
    /// after this window is fully gone from every `WindowManager`
    /// registry — see that method's doc comment for why.
    pub on_removed: Option<teksilo_core::WindowRemovedCallback>,
    /// For a window presenting a modal request (a native modal window, or
    /// a plain window hosting the application tree's), the request's
    /// [`on_dismiss`](teksilo_core::modal::ModalRequest::on_dismiss), run
    /// with this window's context as it closes, whatever closes it: the
    /// dialog's own Cancel path, which a `MessageBox` answers with its
    /// escape button.
    pub(crate) modal_on_dismiss: Option<teksilo_core::overlay::OverlayDismissCallback>,
    /// This window's own dialogs waiting for the user, oldest first: native
    /// dialog windows its tree asked for with no input while it was
    /// minimised. See [`WindowManager::owe_window_dialog`].
    pub(crate) owed_dialogs: Vec<teksilo_core::modal::ModalRequest>,
    /// This window, or a dialog of it, took the focus since its owed dialogs
    /// were last looked at: they are presented in this turn.
    pub(crate) owed_dialogs_due: bool,
    /// The window asked for the user's attention
    /// ([`ask_attention`](Self::ask_attention)) and has not had the focus
    /// since: the request is withdrawn when it, or a dialog of it, takes
    /// the focus.
    pub(crate) attention_asked: bool,
    /// Glyph-atlas content version last uploaded to THIS window's
    /// renderer (`AtlasInfo::version`). Each window compares this
    /// against the shared bridge's current version on its own redraw
    /// and re-uploads when behind — the version model replaces
    /// consume-once dirty semantics so several windows all converge on
    /// the same atlas content. `0` = nothing uploaded yet.
    pub atlas_uploaded_version: u64,
    /// The window's redraw bookkeeping: an undelivered request, whether it
    /// is hidden, a pending non-visual tick; see [`crate::redraw_gate`].
    /// Every request goes through [`ManagedWindow::request_redraw`].
    pub(crate) redraw: crate::redraw_gate::RedrawGate,
    /// This window's copies of the shortcuts the application menu's rows
    /// name; see [`crate::application_menu`].
    pub(crate) app_shortcuts: crate::application_menu::MirroredShortcuts,
}

/// The shortest interval between two `is_minimized` queries of one unfocused
/// window.
pub(crate) const MINIMIZED_PROBE_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(250);

/// How often a window found occluded by asking the platform asks again.
pub(crate) const OCCLUSION_PROBE_INTERVAL: std::time::Duration =
    std::time::Duration::from_millis(500);

/// How often, while nothing on screen can be seen, the session watch rereads
/// the system's state, should the notification that ends it not come.
pub(crate) const SESSION_RECHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

impl ManagedWindow {
    /// Ask winit for a redraw of this window, through the window's
    /// [`RedrawGate`](crate::redraw_gate::RedrawGate). The only way
    /// teksilo-app requests one.
    pub(crate) fn request_redraw(&self) {
        self.redraw.request_with(std::time::Instant::now(), || {
            self.platform_window.request_redraw();
        });
    }

    /// Minimised, fully occluded, or nothing on screen can be seen: the
    /// window draws nothing.
    pub(crate) fn hidden(&self) -> bool {
        self.occluded || self.minimized || self.session_away
    }

    /// Push [`hidden`](Self::hidden) into the redraw gate and act on what
    /// changed: one last frame on the way to hidden (asked of winit
    /// directly, since the gate no longer asks while hidden), one redraw on
    /// the way back.
    pub(crate) fn sync_hidden(&self) {
        self.sync_hidden_with(false);
    }

    /// [`sync_hidden`](Self::sync_hidden) from the redraw handler, before the
    /// frame it runs. That frame is the one either transition asks for: the
    /// last one on the way to hidden, or the reveal; and it serves the draw
    /// wakes dropped while the window drew nothing.
    pub(crate) fn sync_hidden_for_frame(&self) {
        self.sync_hidden_with(true);
    }

    fn sync_hidden_with(&self, frame_runs: bool) {
        let transition = self.redraw.set_hidden(self.hidden());
        self.push_draws_nothing(std::time::Instant::now(), frame_runs);
        if frame_runs {
            return;
        }
        match transition {
            crate::redraw_gate::HiddenTransition::BecameHidden => {
                self.platform_window.request_redraw();
            }
            crate::redraw_gate::HiddenTransition::BecameVisible => self.request_redraw(),
            crate::redraw_gate::HiddenTransition::Unchanged => {}
        }
    }

    /// Tell the window's wake target whether the window draws nothing at
    /// `now`: hidden, or its redraw withheld (see [`crate::redraw_gate`]).
    /// While it does, draw wakes are dropped and one redraw is owed for when
    /// it draws again. Run whenever either half can have changed: on a
    /// hidden change, on a delivered redraw, and every event-loop turn,
    /// since a redraw becomes withheld with time alone.
    pub(crate) fn sync_draws_nothing(&self, now: std::time::Instant) {
        self.push_draws_nothing(now, false);
    }

    /// With `frame_runs`, a window that draws is shown without the redraw
    /// owed for draw wakes dropped meanwhile: the frame about to run serves
    /// them.
    fn push_draws_nothing(&self, now: std::time::Instant, frame_runs: bool) {
        if self.redraw.draws_nothing(now) {
            self.platform_window.set_hidden(true);
        } else if frame_runs {
            self.platform_window.show_for_drawing();
        } else {
            self.platform_window.set_hidden(false);
        }
    }

    /// Bring the window back if it is minimised, and ask for a redraw. The
    /// flag is read as well as asked of winit, since
    /// [`refresh_minimized`](Self::refresh_minimized) reads it at most every
    /// [`MINIMIZED_PROBE_INTERVAL`].
    pub(crate) fn restore(&mut self) {
        let window = self.platform_window.window();
        if self.minimized || window.is_minimized() == Some(true) {
            window.set_minimized(false);
            self.minimized = false;
            self.minimized_probed_at = None;
            self.sync_hidden();
        }
        self.request_redraw();
    }

    /// The window system applied a resize this window asked for at once,
    /// to `size`: the surface follows now, rather than on the `Resized`
    /// that reports it. On macOS winit queues that event, raised inside the
    /// handler that asked, for AppKit's default run-loop mode, which a
    /// quit waiting on its reply to AppKit (a logout, a Dock Quit) never
    /// runs: every frame was drawn at the old size and stretched. The event,
    /// once it comes, finds nothing left to do.
    pub(crate) fn took_own_resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        if self.platform_window.surface_size() == (size.width, size.height) {
            return;
        }
        // The window moved to a display of another scale, whose
        // `ScaleFactorChanged` (held back the same way) has not reached us:
        // the size is in a scale the surface and the layout are not in yet.
        // That event resizes all of them together.
        if self.platform_window.window().scale_factor() != self.platform_window.scale_factor() {
            return;
        }
        self.platform_window.resize(size);
        let sf = self.platform_window.scale_factor();
        let logical_w = (size.width as f64 / sf).round().max(0.0) as u32;
        let logical_h = (size.height as f64 / sf).round().max(0.0) as u32;
        self.state.set_size_from_os((logical_w, logical_h));
        self.request_redraw();
    }

    /// Take the window's own size, when its surface has another: a resize
    /// made inside a handler whose `Resized` winit holds back (see
    /// [`took_own_resize`](Self::took_own_resize)). On macOS winit's
    /// `set_min_inner_size` grows a smaller window itself, so the resize
    /// that follows changes nothing and reports nothing.
    pub(crate) fn take_window_size(&mut self) {
        let size = self.platform_window.window().inner_size();
        if size.width > 0 && size.height > 0 {
            self.took_own_resize(size);
        }
    }

    /// Ask for the user's attention for this window
    /// ([`request_window_attention`]), and remember it, so the request is
    /// [withdrawn](Self::clear_attention) once the user comes to it.
    pub(crate) fn ask_attention(&mut self) {
        self.attention_asked = true;
        request_window_attention(self.platform_window.window());
    }

    /// Withdraw the attention this window asked for, if it asked
    /// ([`clear_window_attention`]): the user came to it. On X11 the
    /// urgency hint stays set until it is withdrawn; elsewhere the request
    /// ends by itself as the window comes to the front, and this is
    /// harmless.
    pub(crate) fn clear_attention(&mut self) {
        if std::mem::take(&mut self.attention_asked) {
            clear_window_attention(self.platform_window.window());
        }
    }

    /// [Restore](Self::restore) the window if teksilo-app knows it
    /// minimised, without asking winit: on X11 the question is a round trip.
    pub(crate) fn restore_known(&mut self) {
        if self.minimized {
            self.platform_window.window().set_minimized(false);
            self.minimized = false;
            self.minimized_probed_at = None;
            self.sync_hidden();
        }
        self.request_redraw();
    }

    /// [Restore](Self::restore) the window, then raise it: winit's
    /// `focus_window` leaves a minimised window where it is, so a raise
    /// alone would not show it.
    pub(crate) fn restore_and_raise(&mut self) {
        self.restore();
        raise_window(self.platform_window.window(), None);
    }

    /// Re-read `minimized` from winit while the window is unfocused, at most
    /// every [`MINIMIZED_PROBE_INTERVAL`]. An X11 iconify sends only
    /// `Focused(false)`, and `is_minimized` costs a round trip there; on
    /// Windows and macOS it is a flag read, and on Wayland it answers `None`,
    /// which reads as not minimised.
    pub(crate) fn refresh_minimized(&mut self, now: std::time::Instant) {
        if self.focused {
            return;
        }
        if self
            .minimized_probed_at
            .is_some_and(|at| now.saturating_duration_since(at) < MINIMIZED_PROBE_INTERVAL)
        {
            return;
        }
        self.minimized_probed_at = Some(now);
        self.minimized = self.platform_window.window().is_minimized() == Some(true);
    }

    /// A frame was skipped at `now`: when the platform can say, and says the
    /// window is not visible at all, mark it occluded, so it draws nothing
    /// and asks for no more frames until it is shown. Whether it now is.
    pub(crate) fn note_occlusion_after_skip(&mut self, now: std::time::Instant) -> bool {
        if self.hidden() || self.platform_window.occluded_now() != Some(true) {
            return self.hidden();
        }
        self.occluded = true;
        self.occlusion_probed_at = Some(now);
        // The frame that would have been owed on the way to hidden cannot be
        // drawn either: the surface just refused it.
        self.sync_hidden_for_frame();
        true
    }

    /// Ask the platform again, every [`OCCLUSION_PROBE_INTERVAL`], about a
    /// window found occluded by asking; shown, it is revealed as winit's
    /// `Occluded(false)` would.
    pub(crate) fn probe_occlusion(&mut self, now: std::time::Instant) {
        let Some(at) = self.occlusion_probed_at else {
            return;
        };
        if now.saturating_duration_since(at) < OCCLUSION_PROBE_INTERVAL {
            return;
        }
        if self.platform_window.occluded_now() == Some(false) {
            self.occluded = false;
            self.occlusion_probed_at = None;
            self.sync_hidden();
        } else {
            self.occlusion_probed_at = Some(now);
        }
    }

    /// When the event loop should next wake for this window: its tree's
    /// next timer deadline, held back while it awaits a redraw it requested
    /// and while it draws nothing. See
    /// [`RedrawGate::deadline`](crate::redraw_gate::RedrawGate::deadline).
    pub(crate) fn wake_deadline(&self, now: std::time::Instant) -> Option<std::time::Instant> {
        let probe = self
            .occlusion_probed_at
            .map(|at| at + OCCLUSION_PROBE_INTERVAL);
        let gate = self.redraw.deadline(now, self.tree.next_timer_deadline());
        if let Some(probe) = probe {
            return Some(gate.map_or(probe, |gate| gate.min(probe)));
        }
        gate
    }
}

/// The routes a window's wake target posts through, as private
/// [`WindowWake`](crate::app::WindowWake) payloads for `window`: the state
/// route, and on a platform that needs it (`draw_off_main`, macOS) the route
/// for draw wakes made off the main thread. `post` sends one payload to the
/// event loop.
pub(crate) fn wake_routes(
    post: impl Fn(Box<dyn std::any::Any + Send>) + Clone + Send + Sync + 'static,
    window: winit::window::WindowId,
    draw_off_main: bool,
) -> (
    std::sync::Arc<dyn Fn() + Send + Sync>,
    Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
) {
    use teksilo_canvas::wake::WakeKind;
    let route = move |kind: WakeKind| {
        let post = post.clone();
        std::sync::Arc::new(move || post(Box::new(crate::app::WindowWake { window, kind })))
            as std::sync::Arc<dyn Fn() + Send + Sync>
    };
    let state = route(WakeKind::Layout);
    let draw = draw_off_main.then(|| route(WakeKind::Draw));
    (state, draw)
}

/// What the manager keeps while it listens to the desktop's appearance: the
/// platform's [`OsThemeWatch`](teksilo_platform::os_theme::OsThemeWatch),
/// which stops when dropped. A trait so the manager's tests can watch a
/// watch start and stop without an event loop or a session bus.
pub(crate) trait DesktopWatch {
    /// Whether a configuration file the theme is read from changed since the
    /// last call; see
    /// [`OsThemeWatch::files_changed`](teksilo_platform::os_theme::OsThemeWatch::files_changed).
    fn files_changed(&mut self) -> bool;
}

impl DesktopWatch for teksilo_platform::os_theme::OsThemeWatch {
    fn files_changed(&mut self) -> bool {
        teksilo_platform::os_theme::OsThemeWatch::files_changed(self)
    }
}

/// How the manager starts a [`DesktopWatch`] posting through `proxy`.
pub(crate) type DesktopWatchStart = fn(Option<&AppEventProxy>) -> Option<Box<dyn DesktopWatch>>;

/// The platform's watch, posting through `proxy`; `None` before the event
/// proxy is installed, and wherever
/// [`OsThemeWatch::new`](teksilo_platform::os_theme::OsThemeWatch::new) answers
/// `None`.
fn start_platform_desktop_watch(proxy: Option<&AppEventProxy>) -> Option<Box<dyn DesktopWatch>> {
    let watch = teksilo_platform::os_theme::OsThemeWatch::new(proxy?.clone())?;
    Some(Box::new(watch))
}

/// Manages multiple application windows.
///
/// Each window owns its own `WidgetTree` and `PlatformWindow`. The manager
/// routes events by winit `WindowId`, broadcasts environment changes, and
/// handles modal dialog blocking.
pub struct WindowManager {
    windows: HashMap<winit::window::WindowId, ManagedWindow>,
    teksilo_to_winit: HashMap<TeksiloWindowId, winit::window::WindowId>,
    /// Stable string-id → id lookup, populated whenever a config carries
    /// `id(...)`. Used by `WindowOps::find_window`.
    string_to_id: HashMap<String, TeksiloWindowId>,
    /// Next allocatable `TeksiloWindowId`. Bumped by `alloc_id`; never
    /// reused after a window closes.
    next_id: u64,
    /// Pending close requests, drained once per tick in
    /// [`process_pending`](Self::process_pending). Each entry records
    /// whether the close is *guarded* (consults the window's close
    /// guard, and may be vetoed) or *forced* (unconditional). See
    /// [`PendingClose`].
    pending_closes: Vec<PendingClose>,
    theme: Theme,
    typesetter: Option<teksilo_text::SharedTypesetter>,
    /// Windows that are blocked by a modal child.
    modal_blocked: HashMap<TeksiloWindowId, TeksiloWindowId>,
    /// Each open modal window's parent. Unlike `modal_blocked`, which holds
    /// one child per parent, it keeps every child of a parent that opened
    /// two, and it is what the quit reads.
    modal_parents: HashMap<TeksiloWindowId, TeksiloWindowId>,
    /// Plain windows opened to present the application tree's modal
    /// requests with no window to parent them: a modal dismissal closes
    /// them as it closes a modal window. See
    /// [`mark_modal_host`](Self::mark_modal_host).
    modal_hosts: std::collections::HashSet<TeksiloWindowId>,
    /// For each open modal window, its owner's minimised state as last seen
    /// and whether the dialog went with it. See
    /// [`follow_owners`](Self::follow_owners).
    owners_seen: HashMap<TeksiloWindowId, OwnerSeen>,
    /// Dialogs raised while the window system hid them, which owe a raise
    /// for when they are shown again. See [`owed_raise`].
    raise_owed: std::collections::HashSet<TeksiloWindowId>,
    /// macOS: dialogs opened while AppKit still reported the window they
    /// block miniaturised (just restored, its animation still running), each
    /// with that window: attached to it as a child window once it is not.
    /// See [`pay_owed_attach`](Self::pay_owed_attach). Empty elsewhere.
    attach_owed: HashMap<TeksiloWindowId, TeksiloWindowId>,
    /// The application tree's dialogs waiting for the user, oldest first:
    /// asked with no input while the window they were to open over was
    /// minimised. See [`owe_app_dialog`](Self::owe_app_dialog).
    owed_app_dialogs: Vec<teksilo_core::modal::ModalRequest>,
    /// A window of the application took the focus since the owed dialogs
    /// were last presented: the user is back, and they are presented.
    owed_app_dialogs_due: bool,
    /// The next modal window [`create_window`](Self::create_window) opens is
    /// raised whatever its owner's focus: an application-tree dialog
    /// presented for the user.
    raise_next_modal: bool,
    /// Whether each window a handler runs in, out of the map meanwhile, had
    /// the focus and was minimised as it was taken out, for a dialog opened
    /// over it. See [`knows_focused`](Self::knows_focused).
    taken_out: HashMap<TeksiloWindowId, TakenOut>,
    /// Windows that took the focus this turn: their modal child is raised
    /// at its end ([`refocus_after_turn`](Self::refocus_after_turn)).
    refocus_due: Vec<TeksiloWindowId>,
    /// The application menu's shortcuts as the application tree declared
    /// them at the last [`sync_application_menu`](Self::sync_application_menu),
    /// with what they were made from, for a window created while the tree is
    /// taken out. See [`crate::application_menu::declarations`].
    app_menu_declarations: Option<(
        crate::application_menu::Source,
        teksilo_core::shortcut::ShortcutRegistry,
    )>,
    /// OS-level accessibility preferences, queried at startup and re-queried
    /// on every window focus gain by
    /// [`refresh_accessibility_preferences`](Self::refresh_accessibility_preferences).
    a11y_prefs: AccessibilityPreferences,
    /// User-controlled global text-scale factor (`1.0` = 100 %). Seeded from
    /// `teksilo_settings::TEXT_SCALE_KEY` before the first window opens and
    /// applied to every tree created afterwards; updated at runtime via
    /// [`set_text_scale`](Self::set_text_scale).
    user_text_scale: f32,
    /// How the app resolves its theme (Manual, FollowSystem, Native).
    theme_mode: ThemeMode,
    pen_batching: teksilo_platform::PenBatching,
    /// The app's own [`ThemeMode::Native`] theme, asked before the default
    /// (IntUI coloured by the desktop). See
    /// [`TeksiloAppBuilder::native_theme`](crate::TeksiloAppBuilder::native_theme).
    native_theme: Option<NativeThemeFactory>,
    /// What [`theme`](Self::theme) was last resolved from while it followed
    /// the OS; `None` once any other theme was set. See
    /// [`ManagedWindow::os_theme`].
    os_theme: Option<OsThemeInputs>,
    /// Whether each window's tree records what the platform adapters
    /// announce ([`WidgetTree::set_records_announcements`]). Off unless the
    /// automation bridge is installed: nothing else in a running application
    /// reads the ring, and the replay behind it costs every sync a second
    /// pass over the tree.
    records_announcements: bool,
    /// Per-tree app context shared with every window's WidgetTree when an
    /// event source is registered on the TeksiloAppBuilder. Each window
    /// receives a clone of this Rc so subscriptions land in a single
    /// shared `subscription_callbacks` map.
    app_context_template: Option<Rc<TreeAppContext>>,
    /// Event-loop proxy used to construct `TitleBarHostCallbacks` when a
    /// window opts into custom chrome. Installed by `TeksiloAppHandler::new`
    /// after the proxy is minted in `TeksiloAppBuilder::run`. `None` during
    /// tests or the headless path, in which case the host's `close()`
    /// callback is a no-op (`TitleBarHostCallbacks::noop`).
    event_proxy: Option<AppEventProxy>,
    /// One-shot callbacks awaiting a `request_activation_token` result, keyed by
    /// the requesting window. Fired (and removed) when the matching
    /// `WindowEvent::ActivationTokenDone` arrives — hands a freshly-minted
    /// xdg-activation token to a child process or an IPC peer. Run on the UI
    /// thread.
    pending_token_callbacks: HashMap<winit::window::WindowId, Box<dyn FnOnce(Option<String>)>>,
    /// Watches whether anything on screen can be seen at all; `None` before
    /// [`watch_session`](Self::watch_session), and on every platform but
    /// macOS.
    session: Option<teksilo_platform::session::SessionWatch>,
    /// On X11, the watch on the `_NET_WM_STATE` of every modal dialog and
    /// its owner, which winit does not report: each change posts a private
    /// [`WmStateChanged`](crate::app::WmStateChanged), answered by
    /// [`reread_wm_states`](Self::reread_wm_states). Started with the first
    /// dialog; see [`sync_wm_state_watch`](Self::sync_wm_state_watch).
    #[cfg(all(unix, not(target_os = "macos")))]
    wm_state_watch: Option<teksilo_platform::x11::WmStateWatch>,
    /// A window to watch was out of the map, dispatching, when the watch was
    /// last synced: synced again when it is put back.
    #[cfg(all(unix, not(target_os = "macos")))]
    wm_state_watch_stale: bool,
    /// How many times [`reread_wm_states`](Self::reread_wm_states) ran: what
    /// a test asserts the watch woke the loop with.
    #[cfg(test)]
    pub(crate) wm_state_rereads: usize,
    /// What [`set_session_away`](Self::set_session_away) last applied to
    /// every window, and what a window created now is born with.
    session_away: bool,
    /// While away, when the watch last reread the system's state.
    session_rechecked_at: Option<std::time::Instant>,
    /// Hears the desktop's appearance change while the theme follows the
    /// OS; `None` under [`ThemeMode::Manual`], before
    /// [`sync_os_theme_watch`](Self::sync_os_theme_watch) has run with the
    /// event proxy installed, and on every platform but Linux.
    os_theme_watch: Option<Box<dyn DesktopWatch>>,
    /// What starts [`os_theme_watch`](Self::os_theme_watch):
    /// [`start_platform_desktop_watch`], except in tests.
    start_os_theme_watch: DesktopWatchStart,
    /// The order in which the open windows were opened or focused; see
    /// [`most_recent_window`](Self::most_recent_window).
    recency: crate::window_recency::WindowRecency,
    /// The application tree: a widget tree no window hosts, for code that
    /// must run with no window open. Never painted, no `WindowState`, no
    /// accessibility adapter. Taken out while one of its handlers runs,
    /// like a window's tree. See [`install_app_tree`](Self::install_app_tree).
    app_tree: Option<WidgetTree>,
    /// Set by the application tree's waker when it posts an
    /// [`AppTreeWake`](crate::app::AppTreeWake), cleared when the loop takes
    /// it: at most one is in flight.
    app_tree_wake_pending: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// An activation token the event being routed carried (see
    /// [`WithActivationToken`](crate::WithActivationToken)), for the first
    /// window operation of its handler that raises or opens a window to
    /// use; also the token an intent routed from another tree carried, for
    /// the actions it reaches ([`WindowOps::hold_activation_token`](teksilo_core::WindowOps::hold_activation_token)).
    /// Cleared at the end of the `post_event` that follows the dispatch,
    /// used or not.
    dispatch_activation_token: Option<String>,
    /// What [`dispatch_activation_token`](Self::dispatch_activation_token)
    /// left once its handler returned: only a `WindowState::focus()` with no
    /// token of its own, applied in this turn's command drain, takes it, so
    /// a window the framework opens meanwhile (a native dialog, the
    /// application tree's tick) cannot.
    focus_command_token: Option<String>,
    /// This turn dispatched the user's input: a click, a key, a contact, a
    /// native menu row, a screen reader's action. What a handler asks for in
    /// it is the user's request; see [`acts_for_user`](Self::acts_for_user).
    /// Cleared at the end of the turn's `post_event`.
    user_activation: bool,
    /// The activation tokens window operations used, oldest first: what a
    /// test asserts a token reached.
    #[cfg(test)]
    pub(crate) used_activation_tokens: Vec<String>,
    /// Intents a window left unclaimed and could not hand over before it
    /// closed (a close guard's, sent as the window went), each with the
    /// chain of windows it was a modal dialog of, as it was then: they still
    /// go to those windows, nearest first, then to the application tree.
    orphan_intents: Vec<(teksilo_core::Intent, Vec<TeksiloWindowId>)>,
    /// What the application tree takes, shared with every window's tree so
    /// a shortcut nothing in a window takes goes to it, and published from
    /// the application tree whenever it has run. See
    /// [`ApplicationScope`](teksilo_core::ApplicationScope).
    application_scope: teksilo_core::ApplicationScope,
    /// Bumped whenever [`theme`](Self::theme) changes.
    theme_generation: u64,
    /// The [`theme_generation`](Self::theme_generation) the application
    /// tree was last given, so a change made while the tree was taken out
    /// (a window opened from one of its handlers resolving the OS theme)
    /// reaches it when it is put back.
    app_tree_theme_generation: u64,
    /// The closes guards deferred
    /// ([`CloseResponse::Defer`]),
    /// by window, waiting on their answer; settled by
    /// [`process_pending`](Self::process_pending). A window closing by any
    /// route drops its entry, and a late answer then changes nothing.
    deferred_closes: HashMap<TeksiloWindowId, teksilo_core::window::CloseDecisionReceipt>,
    /// The application's quit guard, from
    /// [`TeksiloAppBuilder::on_quit_requested`](crate::TeksiloAppBuilder::on_quit_requested),
    /// asked in the application tree before any window.
    app_quit_guard: Option<CloseGuard>,
    /// The answer the application's quit guard deferred, while the quit
    /// waits on it.
    app_quit_deferral: Option<teksilo_core::window::CloseDecisionReceipt>,
    /// The quit under way, if any; see [`crate::quit`].
    quit: Option<crate::quit::PendingQuit>,
    /// Told how the quit under way ends.
    quit_replies: Vec<crate::quit::QuitReply>,
    /// The quit under way has reported running into its step bound; see
    /// [`advance_quit`](Self::advance_quit).
    quit_bound_reported: bool,
    /// The last [`PendingQuit::generation`](crate::quit::PendingQuit) handed
    /// out.
    quit_generation: u64,
    /// A quit ran to completion: the application ends whatever its quit
    /// policy.
    quit_completed: bool,
}

// Doc comments on **every** method here are enforced, not hoped for: a new item
// inserted between an existing doc comment and the item it documented — which
// has happened here, to `set_theme_mode` — leaves the displaced item
// undocumented, and these are the lints that see it. `deny` rather than `warn`
// because this crate does not set `missing_docs` at the root, so a warning
// would sit in a build log nobody reads.
//
// **Two lints, because one does not cover the class.** `missing_docs` is
// defined not to fire on a private item, and this block is roughly half
// `pub(crate)` — so a displaced `pub(crate)` neighbour passed it silently, which
// is the same defect wearing a narrower visibility. Clippy's
// `missing_docs_in_private_items` is the half `missing_docs` leaves out; it is a
// tool lint, so plain `rustc` ignores it and the workspace clippy gate
// (`-D warnings`) is what makes it bite. Verified by deleting the doc on
// `take_activation_token_callback`, which `missing_docs` alone compiled clean.
#[deny(missing_docs, clippy::missing_docs_in_private_items)]
impl WindowManager {
    /// An empty manager whose windows will be built against `theme`.
    pub fn new(theme: Theme) -> Self {
        let a11y_prefs = AccessibilityPreferences::query();
        Self {
            windows: HashMap::new(),
            teksilo_to_winit: HashMap::new(),
            string_to_id: HashMap::new(),
            next_id: 1,
            pending_closes: Vec::new(),
            theme,
            typesetter: None,
            modal_blocked: HashMap::new(),
            modal_parents: HashMap::new(),
            modal_hosts: Default::default(),
            owners_seen: HashMap::new(),
            raise_owed: Default::default(),
            attach_owed: HashMap::new(),
            owed_app_dialogs: Vec::new(),
            owed_app_dialogs_due: false,
            raise_next_modal: false,
            taken_out: HashMap::new(),
            refocus_due: Vec::new(),
            app_menu_declarations: None,
            a11y_prefs,
            user_text_scale: 1.0,
            theme_mode: ThemeMode::Manual,
            pen_batching: teksilo_platform::PenBatching::default(),
            native_theme: None,
            os_theme: None,
            records_announcements: false,
            app_context_template: None,
            event_proxy: None,
            pending_token_callbacks: HashMap::new(),
            session: None,
            #[cfg(all(unix, not(target_os = "macos")))]
            wm_state_watch: None,
            #[cfg(all(unix, not(target_os = "macos")))]
            wm_state_watch_stale: false,
            #[cfg(test)]
            wm_state_rereads: 0,
            session_away: false,
            session_rechecked_at: None,
            os_theme_watch: None,
            start_os_theme_watch: start_platform_desktop_watch,
            recency: crate::window_recency::WindowRecency::default(),
            app_tree: None,
            app_tree_wake_pending: Default::default(),
            dispatch_activation_token: None,
            focus_command_token: None,
            user_activation: false,
            #[cfg(test)]
            used_activation_tokens: Vec::new(),
            orphan_intents: Vec::new(),
            application_scope: teksilo_core::ApplicationScope::new(),
            theme_generation: 0,
            app_tree_theme_generation: 0,
            deferred_closes: HashMap::new(),
            app_quit_guard: None,
            app_quit_deferral: None,
            quit: None,
            quit_replies: Vec::new(),
            quit_bound_reported: false,
            quit_generation: 0,
            quit_completed: false,
        }
    }

    /// Store a one-shot callback fired when this window's pending
    /// `request_activation_token` resolves via `WindowEvent::ActivationTokenDone`.
    pub(crate) fn store_activation_token_callback(
        &mut self,
        id: winit::window::WindowId,
        cb: Box<dyn FnOnce(Option<String>)>,
    ) {
        self.pending_token_callbacks.insert(id, cb);
    }

    /// Take the pending activation-token callback for `id`, if any.
    pub(crate) fn take_activation_token_callback(
        &mut self,
        id: winit::window::WindowId,
    ) -> Option<Box<dyn FnOnce(Option<String>)>> {
        self.pending_token_callbacks.remove(&id)
    }

    /// How a drained pen batch reaches each window's tree. Applied to every
    /// window created after this call. See
    /// [`PenBatching`](teksilo_platform::PenBatching).
    pub fn set_pen_batching(&mut self, mode: teksilo_platform::PenBatching) {
        self.pen_batching = mode;
    }

    /// The app's own [`ThemeMode::Native`] theme, asked before the default
    /// whenever Native mode resolves a theme. See
    /// [`resolve_native_theme`](crate::resolve_native_theme).
    pub(crate) fn set_native_theme(&mut self, factory: Option<NativeThemeFactory>) {
        self.native_theme = factory;
    }

    /// Whether the windows opened from now on record announcements. See
    /// [`WidgetTree::set_records_announcements`].
    pub(crate) fn set_records_announcements(&mut self, record: bool) {
        self.records_announcements = record;
    }

    /// The tree a new window starts from. A tree built with
    /// `WidgetTree::new()` records announcements, for the headless trees
    /// tests build; a window's records them only for the automation bridge.
    fn new_window_tree(&self, theme: Theme) -> WidgetTree {
        let mut tree = WidgetTree::new().with_theme(theme);
        tree.set_records_announcements(self.records_announcements);
        // What no action of a tree takes is offered to the other side, a
        // shortcut's when the application tree takes it; see
        // `crate::intent_routing`.
        tree.set_records_unclaimed_intents(true);
        tree.set_application_scope(Some(self.application_scope.clone()));
        // The standard edit shortcuts arrive with English names; the settings
        // UI shows them in the user's language.
        teksilo_widgets::localize_standard_shortcuts(tree.shortcut_registry_mut());
        tree
    }

    /// Give a new tree what every tree of this application shares: the
    /// active locale and layout direction, the app context, the OS
    /// accessibility preferences and the user's text scale. Shared by the
    /// windows' trees and the [application tree](Self::install_app_tree).
    ///
    /// The locale must be seeded BEFORE the root builder runs so any `tr!`
    /// call in `build()` sees it on the first build, and BEFORE the first
    /// layout so the tree's `layout_direction` already matches the
    /// manager's. Without it `WidgetTree::new()` is left-to-right with an
    /// empty locale, and an app started in an RTL locale would lay out its
    /// first window left to right until the user switched locale.
    fn seed_tree(&self, tree: &mut WidgetTree) {
        if let Some((loc, dir)) = teksilo_i18n::thread_local::with_active(|m| {
            (
                m.locale_signal().get().to_string(),
                m.direction_signal().get(),
            )
        }) {
            tree.set_layout_direction(dir);
            tree.set_locale(loc);
        }
        if let Some(template) = self.app_context_template.as_ref() {
            tree.set_app_context(template.clone());
        }
        tree.set_accessibility_preferences(
            self.a11y_prefs.high_contrast,
            self.a11y_prefs.reduced_motion,
            self.a11y_prefs.text_scale_factor,
        );
        tree.set_screen_reader_state(self.a11y_prefs.screen_reader);
        if (self.user_text_scale - 1.0).abs() > f32::EPSILON {
            tree.set_user_text_scale(self.user_text_scale);
        }
    }

    /// Create the application tree and build its root with `root`, which
    /// returns the root's id. Replaces any application tree already there.
    ///
    /// The tree is seeded as a window's is (locale and direction, app
    /// context, accessibility preferences, text scale) and themed with the
    /// application's theme, but has no `WindowState`
    /// (so `ctx.window()` is `None` in its handlers), no app-wide
    /// post-root wrapper, no accessibility adapter, and records no
    /// announcements. It shares the typesetter, so a widget in it can
    /// measure text.
    pub fn install_app_tree(&mut self, root: impl FnOnce(&mut WidgetTree) -> WidgetId) -> WidgetId {
        // A theme that follows the OS is otherwise resolved by the first
        // window, which an application started with none has not opened.
        if self.theme_mode != ThemeMode::Manual {
            self.new_window_theme(None, OsQueries::PLATFORM);
        }
        let mut tree = self.new_window_tree(self.theme.clone());
        tree.set_records_announcements(false);
        // It is the application scope, not one of its readers.
        tree.set_application_scope(None);
        self.seed_tree(&mut tree);
        if let Some(typesetter) = self.typesetter.as_ref() {
            tree = tree.with_text_backend(typesetter.as_text_backend());
        }
        // Before the root builds, so a source attached in `build()` already
        // wakes the loop for this tree.
        if let Some(proxy) = self.event_proxy.clone() {
            tree.set_redraw_waker(Some(std::sync::Arc::new(crate::app::AppTreeWaker {
                post: std::sync::Arc::new(move |payload| proxy.send_external_boxed(payload)),
                pending: self.app_tree_wake_pending.clone(),
            })));
        }
        let root_id = root(&mut tree);
        tree.publish_application_scope(&self.application_scope);
        self.app_tree = Some(tree);
        self.app_tree_theme_generation = self.theme_generation;
        root_id
    }

    /// Give the application tree the application's theme if it changed
    /// since the tree last got it. A no-op while the tree is taken out:
    /// [`reinsert_app_tree`](Self::reinsert_app_tree) catches up.
    fn sync_app_tree_theme(&mut self) {
        if self.app_tree_theme_generation != self.theme_generation
            && let Some(tree) = &mut self.app_tree
        {
            tree.set_theme(self.theme.clone());
            self.app_tree_theme_generation = self.theme_generation;
        }
    }

    /// Every window's tree, then the application tree: the trees a request
    /// that applies application-wide (locale, theme, text scale) can come
    /// from.
    fn trees_mut(&mut self) -> impl Iterator<Item = &mut WidgetTree> {
        self.windows
            .values_mut()
            .map(|managed| &mut managed.tree)
            .chain(self.app_tree.as_mut())
    }

    /// Take the application tree out while one of its handlers runs with
    /// `&mut self` behind a [`WindowOpsImpl::windowless`] sink; put it back
    /// with [`reinsert_app_tree`](Self::reinsert_app_tree).
    pub(crate) fn take_app_tree(&mut self) -> Option<WidgetTree> {
        self.app_tree.take()
    }

    /// Put back the tree [`take_app_tree`](Self::take_app_tree) took.
    /// The tree has run, so what it takes is published again for the
    /// windows' shortcuts.
    pub(crate) fn reinsert_app_tree(&mut self, tree: WidgetTree) {
        tree.publish_application_scope(&self.application_scope);
        self.app_tree = Some(tree);
        self.sync_app_tree_theme();
    }

    /// Hold `token` for the handler about to run: the first window it
    /// raises or opens uses it. See
    /// [`WithActivationToken`](crate::WithActivationToken).
    pub(crate) fn set_dispatch_activation_token(&mut self, token: String) {
        self.dispatch_activation_token = Some(token);
    }

    /// Take the routed event's activation token, if it carried one and no
    /// window operation took it yet. Its handler's operations only.
    pub(crate) fn take_dispatch_activation_token(&mut self) -> Option<String> {
        self.dispatch_activation_token.take()
    }

    /// [`take_dispatch_activation_token`](Self::take_dispatch_activation_token)
    /// for a window operation that activates a window with it (opening or
    /// raising one). A test build records the tokens used.
    pub(crate) fn use_dispatch_activation_token(&mut self) -> Option<String> {
        let token = self.take_dispatch_activation_token();
        #[cfg(test)]
        if let Some(token) = &token {
            self.used_activation_tokens.push(token.clone());
        }
        token
    }

    /// Whether the routed event's activation token is still held for its
    /// handler's operations.
    pub(crate) fn holds_dispatch_activation_token(&self) -> bool {
        self.dispatch_activation_token.is_some()
    }

    /// The routed event's handler returned: a token it did not use waits,
    /// for this turn's command drain only, for a `WindowState::focus()`
    /// it queued. A token already waiting there stays: an earlier handler of
    /// this turn left it for the focus it queued, and the first focus the
    /// drain applies is that one. So a handler that used its token, never
    /// had one, or released one unused after it (an intent routed on from
    /// that handler, whose actions raised nothing) does not replace it.
    pub(crate) fn end_dispatch_activation(&mut self) {
        if let Some(token) = self.dispatch_activation_token.take()
            && self.focus_command_token.is_none()
        {
            self.focus_command_token = Some(token);
        }
    }

    /// Drop the routed event's activation token, used or not: a token is
    /// good for the activation the event asked for, and that is over.
    pub(crate) fn clear_dispatch_activation_token(&mut self) {
        self.dispatch_activation_token = None;
        self.focus_command_token = None;
    }

    /// This turn dispatches the user's input; see
    /// [`acts_for_user`](Self::acts_for_user).
    pub(crate) fn note_user_activation(&mut self) {
        self.user_activation = true;
    }

    /// The turn is over: what is asked from now on is not the user's input's.
    pub(crate) fn clear_user_activation(&mut self) {
        self.user_activation = false;
    }

    /// Whether what is asked now comes from the user: the turn dispatched
    /// the user's input ([`note_user_activation`](Self::note_user_activation)),
    /// or carries an activation token (a launcher's, through
    /// [`WithActivationToken`](crate::WithActivationToken), or one an intent
    /// routed from another tree brought). Only then may Teksilo take the
    /// focus from another application to show a window.
    pub(crate) fn acts_for_user(&self) -> bool {
        self.user_activation
            || self.dispatch_activation_token.is_some()
            || self.focus_command_token.is_some()
    }

    /// Take the intents of windows that closed before handing theirs over,
    /// each with the chain of windows it was a modal dialog of.
    pub(crate) fn take_orphan_intents(
        &mut self,
    ) -> Vec<(teksilo_core::Intent, Vec<TeksiloWindowId>)> {
        std::mem::take(&mut self.orphan_intents)
    }

    /// Take what every window left unclaimed, oldest window's first within
    /// the map's order, each intent with the chain of windows its window is
    /// a modal dialog of ([`owner_chain`](Self::owner_chain)); the closed
    /// windows' leftovers come first.
    pub(crate) fn take_unclaimed_window_intents(
        &mut self,
    ) -> Vec<(teksilo_core::Intent, Vec<TeksiloWindowId>)> {
        let mut intents = self.take_orphan_intents();
        let ids: Vec<TeksiloWindowId> = self.windows.values().map(|w| w.teksilo_id).collect();
        for id in ids {
            let owners = self.owner_chain(id);
            if let Some(managed) = self.get_by_teksilo_mut(id) {
                intents.extend(
                    managed
                        .tree
                        .take_unclaimed_intents()
                        .into_iter()
                        .map(|intent| (intent, owners.clone())),
                );
            }
        }
        intents
    }

    /// The windows `teksilo_id` is a modal dialog of, nearest first: the
    /// window it blocks, the one that window blocks, and so on up. Empty for
    /// a window that is no dialog. Recorded with a dialog's leftover intents
    /// when they are taken, so a window in between that closes meanwhile
    /// does not cut the ones above it off.
    pub(crate) fn owner_chain(&self, teksilo_id: TeksiloWindowId) -> Vec<TeksiloWindowId> {
        let mut chain = Vec::new();
        let mut owner = self.modal_parent_of(teksilo_id);
        while let Some(up) = owner
            && chain.len() < 64
        {
            chain.push(up);
            owner = self.modal_parent_of(up);
        }
        chain
    }

    /// Whether a turn owes another pass of the drains: an intent left
    /// unclaimed anywhere (an orphan's included), or a request the
    /// application tree or a window made since the drains ran. Window mount
    /// actions are not counted: a modal-blocked window keeps them on
    /// purpose.
    pub(crate) fn owes_turn_work(&self) -> bool {
        !self.orphan_intents.is_empty()
            || self.has_pending_application_menu_rows()
            || self.windows.values().any(|managed| {
                managed.tree.has_unclaimed_intents() || managed.tree.has_pending_dispatch_requests()
            })
            || self
                .app_tree
                .as_ref()
                .is_some_and(|tree| tree.has_unclaimed_intents() || tree.has_pending_app_requests())
    }

    /// Record that `teksilo_id` is a plain window presenting a modal
    /// request of the application tree, which its dismissal closes.
    pub(crate) fn mark_modal_host(&mut self, teksilo_id: TeksiloWindowId) {
        self.modal_hosts.insert(teksilo_id);
    }

    /// Take the application tree's posted wake: whether one was pending.
    /// The waker posts again only after this.
    pub(crate) fn take_app_tree_wake(&self) -> bool {
        self.app_tree_wake_pending
            .swap(false, std::sync::atomic::Ordering::AcqRel)
    }

    /// The application menu's holder, registered in app-state by the
    /// builder.
    pub(crate) fn application_menu(&self) -> Option<teksilo_widgets::menu::ApplicationMenu> {
        self.app_context_template.as_ref().and_then(|t| {
            t.app_state::<teksilo_widgets::menu::ApplicationMenu>()
                .cloned()
        })
    }

    /// Bring one window's copies of the application menu's shortcuts in
    /// line with the menu and the application tree's declarations; see
    /// [`crate::application_menu`]. A no-op with no application tree.
    fn sync_window_shortcuts(
        &self,
        mirrored: &mut crate::application_menu::MirroredShortcuts,
        tree: &mut WidgetTree,
    ) {
        let Some(menu) = self.application_menu() else {
            return;
        };
        match self.app_tree.as_ref() {
            Some(app_tree) => {
                let (generation, version) = menu.revision();
                let app = app_tree.shortcut_registry();
                mirrored.sync(
                    (generation, version, app.version().get()),
                    || menu.shortcut_ids(),
                    app,
                    tree.shortcut_registry_mut(),
                );
            }
            // Taken out while one of its handlers runs, which is when an
            // action of the application tree opens a window: the copies
            // come from what it declared as the turn began.
            None => {
                if let Some((source, declared)) = &self.app_menu_declarations {
                    mirrored.sync(
                        *source,
                        || menu.shortcut_ids(),
                        declared,
                        tree.shortcut_registry_mut(),
                    );
                }
            }
        }
    }

    /// Bring what the application menu reaches in line with the menu and the
    /// application tree's declarations: every window's copies of the
    /// shortcuts its rows name, and the native application menu, when the
    /// app installed a native menu. A user's rebind of a copy, made in a
    /// window's `ShortcutSettings`, goes to the application tree's registry
    /// first, so every window and the native menu follow it (see
    /// [`crate::application_menu::sync_all`]). Cheap when nothing changed, so
    /// the loop calls it after every turn's handlers. A no-op with no
    /// application tree.
    pub(crate) fn sync_application_menu(&mut self) {
        let (Some(menu), Some(app_tree)) = (self.application_menu(), self.app_tree.as_mut()) else {
            return;
        };
        let mut windows: Vec<_> = self
            .windows
            .values_mut()
            .map(|managed| {
                (
                    &mut managed.app_shortcuts,
                    managed.tree.shortcut_registry_mut(),
                )
            })
            .collect();
        crate::application_menu::sync_all(
            menu.revision(),
            || menu.shortcut_ids(),
            app_tree.shortcut_registry_mut(),
            &mut windows,
        );
        drop(windows);
        let app = app_tree.shortcut_registry();
        let (generation, version) = menu.revision();
        let source = (generation, version, app.version().get());
        if self
            .app_menu_declarations
            .as_ref()
            .is_none_or(|(made_from, _)| *made_from != source)
        {
            self.app_menu_declarations = Some((
                source,
                crate::application_menu::declarations(menu.shortcut_ids(), app),
            ));
        }
        let template = self.app_context_template.as_ref();
        if let (Some(handle), Some(poster)) = (
            template.and_then(|t| t.app_state::<teksilo_platform::native_menu::NativeMenuHandle>()),
            template.and_then(|t| t.poster()),
        ) {
            menu.sync_native(handle, poster.clone(), app.version().get(), &|id| {
                app.effective(id).and_then(|shortcut| shortcut.primary)
            });
        }
    }

    /// Grey out or enable again the native menu's rows whose only effect is
    /// their intent, from [`native_menu_handled_intents`](Self::native_menu_handled_intents):
    /// what the route a chosen row's intent takes answers now. A no-op
    /// without a native menu, or with one that shows no bar (every platform
    /// but macOS); the handle evaluates no row when the answer is the last
    /// one's. See [`NativeMenuHandle::refresh_availability`](teksilo_platform::native_menu::NativeMenuHandle::refresh_availability).
    pub(crate) fn refresh_native_menu_availability(&self) {
        if let Some(handle) = self
            .app_context_template
            .as_ref()
            .and_then(|t| t.app_state::<teksilo_platform::native_menu::NativeMenuHandle>())
            .filter(|handle| handle.shows_bar())
        {
            handle.refresh_availability(&self.native_menu_handled_intents());
        }
    }

    /// Every intent a row chosen in the native menu bar would have handled,
    /// sorted: what [`native_menu_command_handled`](Self::native_menu_command_handled)
    /// answers true for, with no fallback window, in one walk.
    fn native_menu_handled_intents(&self) -> Vec<&'static str> {
        match self
            .focused_window()
            .and_then(|id| self.teksilo_to_winit.get(&id))
            .and_then(|winit_id| self.windows.get(winit_id))
        {
            Some(managed) => crate::native_menu_route::native_menu_handled_intents(&managed.tree),
            None => {
                let mut names = self.application_scope.taken();
                names.sort_unstable();
                names.dedup();
                names
            }
        }
    }

    /// Whether an intent named `name`, chosen in the native menu bar, would
    /// be handled: by the focused window (else `fallback`, the window whose
    /// own menu the row is in), from its focused widget as a native row sends it, its
    /// application scope included; by the application tree when there is
    /// no such window. What the App menu's Quit asks before sending its
    /// intent.
    pub(crate) fn native_menu_command_handled(
        &self,
        name: &str,
        fallback: Option<TeksiloWindowId>,
    ) -> bool {
        match self
            .focused_window()
            .or(fallback)
            .and_then(|id| self.teksilo_to_winit.get(&id))
            .and_then(|winit_id| self.windows.get(winit_id))
        {
            Some(managed) => crate::native_menu_route::native_menu_handles(&managed.tree, name),
            None => self.application_scope.takes(name),
        }
    }

    /// Whether a row of the application menu chosen in a window waits for
    /// the application tree to run its closure.
    pub(crate) fn has_pending_application_menu_rows(&self) -> bool {
        self.application_menu()
            .is_some_and(|menu| menu.has_pending())
    }

    /// The application tree, if one was installed.
    pub fn app_tree(&self) -> Option<&WidgetTree> {
        self.app_tree.as_ref()
    }

    /// The application tree, mutably.
    pub fn app_tree_mut(&mut self) -> Option<&mut WidgetTree> {
        self.app_tree.as_mut()
    }

    /// Set the theme mode (called by TeksiloAppHandler during initialization).
    /// Once the event loop runs, the desktop's appearance is watched while
    /// the mode follows the OS.
    pub fn set_theme_mode(&mut self, mode: ThemeMode) {
        self.theme_mode = mode;
        self.sync_os_theme_watch();
    }

    /// Start the desktop's appearance watch while the theme follows the OS
    /// ([`ThemeMode::Native`] or [`ThemeMode::FollowSystem`]), and stop it
    /// under [`ThemeMode::Manual`]. Each change it hears posts an
    /// [`OsThemeChanged`](teksilo_platform::os_theme::OsThemeChanged), which
    /// the loop answers with
    /// [`route_os_theme_payload`](Self::route_os_theme_payload). A no-op
    /// before the event proxy is installed, and while already in step.
    pub(crate) fn sync_os_theme_watch(&mut self) {
        if self.theme_mode == ThemeMode::Manual {
            self.os_theme_watch = None;
            return;
        }
        if self.os_theme_watch.is_some() {
            return;
        }
        self.os_theme_watch = (self.start_os_theme_watch)(self.event_proxy.as_ref());
    }

    /// Answer `payload` when it is an
    /// [`OsThemeChanged`](teksilo_platform::os_theme::OsThemeChanged): read
    /// the desktop again and give each window the theme it now resolves to
    /// ([`reapply_os_theme_from`](Self::reapply_os_theme_from)), and refresh
    /// the accessibility preferences, among them reduced motion from KDE's
    /// animation speed. `Ok` with whether anything changed, so the caller
    /// redraws; any other payload is handed back.
    pub(crate) fn route_os_theme_payload(
        &mut self,
        payload: Box<dyn std::any::Any + Send>,
    ) -> Result<bool, Box<dyn std::any::Any + Send>> {
        self.route_os_theme_payload_from(payload, OsQueries::PLATFORM)
    }

    /// [`route_os_theme_payload`](Self::route_os_theme_payload), reading the
    /// desktop and its accessibility preferences through `queries`.
    fn route_os_theme_payload_from(
        &mut self,
        payload: Box<dyn std::any::Any + Send>,
        queries: OsQueries,
    ) -> Result<bool, Box<dyn std::any::Any + Send>> {
        payload.downcast::<teksilo_platform::os_theme::OsThemeChanged>()?;
        let themed = self.reapply_os_theme_from(queries);
        let preferences = self.refresh_accessibility_preferences_from(queries.accessibility);
        Ok(themed || preferences)
    }

    /// Read the desktop again through `queries` and give each window the
    /// theme it now resolves to, as [`apply_os_theme`](Self::apply_os_theme)
    /// does with no winit hint; whether any theme was set. The watch's
    /// file times are taken first, so the change that prompted this is not
    /// found again when a window next takes focus.
    fn reapply_os_theme_from(&mut self, queries: OsQueries) -> bool {
        if let Some(watch) = &mut self.os_theme_watch {
            watch.files_changed();
        }
        self.apply_os_theme_from(None, queries)
    }

    /// The backstop for a session with no bus or a change signal that never
    /// came, for when a window takes focus: when a file of the `kdeglobals`
    /// or `breezerc` cascade changed since the watch last looked, read the
    /// desktop again and apply what it resolves to. Whether any theme was
    /// set; `false` with no watch.
    pub(crate) fn recheck_os_theme_files(&mut self) -> bool {
        let Some(watch) = &mut self.os_theme_watch else {
            return false;
        };
        if !watch.files_changed() {
            return false;
        }
        self.apply_os_theme_from(None, OsQueries::PLATFORM)
    }

    /// Install the event-loop proxy (called by TeksiloAppHandler once the
    /// proxy is available). Enables `TitleBarHostCallbacks::request_close`
    /// to post `CloseWindowRequest` back through the event loop.
    pub fn set_event_proxy(&mut self, proxy: AppEventProxy) {
        self.event_proxy = Some(proxy);
    }

    /// The event-loop proxy, once installed.
    pub(crate) fn event_proxy(&self) -> Option<&AppEventProxy> {
        self.event_proxy.as_ref()
    }

    /// Start the session watch, on the main thread, once the event proxy is
    /// installed: each change it hears posts a private
    /// [`SessionChanged`](crate::app::SessionChanged), which the loop answers
    /// with [`apply_session`](Self::apply_session). A no-op once watching,
    /// and on every platform but macOS.
    pub(crate) fn watch_session(&mut self) {
        if self.session.is_some() {
            return;
        }
        let Some(proxy) = self.event_proxy.clone() else {
            return;
        };
        self.session = teksilo_platform::session::SessionWatch::new(move || {
            teksilo_core::AppEventPoster::post_external(
                &proxy,
                Box::new(crate::app::SessionChanged),
            );
        });
    }

    /// Apply what the session watch says now; see
    /// [`set_session_away`](Self::set_session_away).
    pub(crate) fn apply_session(&mut self, now: std::time::Instant) {
        let away = self
            .session
            .as_ref()
            .is_some_and(teksilo_platform::session::SessionWatch::away);
        self.set_session_away(away, now);
    }

    /// Hide every window while nothing on screen can be seen, and show them
    /// again when that ends, each with the one redraw a reveal owes. A
    /// window created meanwhile is born with it. While away, the watch
    /// rereads the system every [`SESSION_RECHECK_INTERVAL`]
    /// ([`recheck_session`](Self::recheck_session)).
    pub(crate) fn set_session_away(&mut self, away: bool, now: std::time::Instant) {
        self.session_rechecked_at = away.then_some(now);
        if away == self.session_away {
            return;
        }
        self.session_away = away;
        for managed in self.windows.values_mut() {
            managed.session_away = away;
            managed.sync_hidden();
        }
    }

    /// While away, have the watch reread the system's state every
    /// [`SESSION_RECHECK_INTERVAL`] and clear what it no longer reports: a
    /// notification that never came would otherwise leave every window
    /// drawing nothing.
    pub(crate) fn recheck_session(&mut self, now: std::time::Instant) {
        let Some(at) = self.session_rechecked_at else {
            return;
        };
        if now.saturating_duration_since(at) < SESSION_RECHECK_INTERVAL {
            return;
        }
        if let Some(session) = &self.session {
            session.recheck();
        }
        self.apply_session(now);
    }

    /// When [`recheck_session`](Self::recheck_session) is next due; `None`
    /// while something can be seen.
    pub(crate) fn session_deadline(&self) -> Option<std::time::Instant> {
        self.session_rechecked_at
            .map(|at| at + SESSION_RECHECK_INTERVAL)
    }

    /// Install the per-tree app context template that every newly created
    /// window's WidgetTree should adopt. Called by TeksiloAppHandler when the
    /// application registered an event source on the builder.
    pub fn set_app_context_template(&mut self, template: Rc<TreeAppContext>) {
        self.app_context_template = Some(template);
    }

    /// The shared per-tree app context, if any, used by TeksiloAppHandler to
    /// look up subscription callbacks when delivering
    /// `AppEvent::SubscriptionEvent` to the UI thread.
    pub(crate) fn app_context_template(&self) -> Option<&Rc<TreeAppContext>> {
        self.app_context_template.as_ref()
    }

    /// Get the current theme mode.
    pub fn theme_mode(&self) -> ThemeMode {
        self.theme_mode
    }

    /// Recompute and broadcast the theme from the **current OS appearance**,
    /// per the active theme mode. A no-op under `Manual`. Under `Native` it
    /// resolves the desktop's report through
    /// [`resolve_native_theme`](crate::resolve_native_theme): the app's own
    /// native theme, else IntUI in the OS's actual colours (GNOME/KDE/Cinnamon
    /// on Linux); under `FollowSystem` it picks the built-in light/dark
    /// preset. Both results carry the id `"system"`.
    ///
    /// Each window keeps its target density: its theme is resolved at the
    /// density its tree is at, and the theme new windows are built with at
    /// the density of the manager's current theme. A window whose desktop
    /// report, light or dark state and density are those its theme was last
    /// resolved from is not given its theme again.
    ///
    /// `os_dark_hint` is the OS light/dark state as reported by winit (e.g.
    /// from a `WindowEvent::ThemeChanged`), used where the desktop states no
    /// light or dark preference itself (macOS / Windows, where
    /// `query_os_theme_colors()` returns `NoPreference`). On a Linux desktop
    /// that states one the hint is unused. Pass `None` when no winit signal is
    /// available (e.g. a runtime "follow system" request): the current
    /// window's reported theme is used where possible, otherwise it resolves
    /// to light.
    pub fn apply_os_theme(&mut self, os_dark_hint: Option<bool>) {
        self.apply_os_theme_from(os_dark_hint, OsQueries::PLATFORM);
    }

    /// [`apply_os_theme`](Self::apply_os_theme), reading the desktop through
    /// `queries`; whether any theme was set.
    fn apply_os_theme_from(&mut self, os_dark_hint: Option<bool>, queries: OsQueries) -> bool {
        // Fall back to a window's currently-reported winit theme when the
        // caller didn't supply a hint (so picking "System" on macOS/Windows
        // adopts the right light/dark immediately, not just on the next toggle).
        let hint = os_dark_hint.or_else(|| {
            self.windows
                .values()
                .next()
                .and_then(|m| m.platform_window.window().theme())
                .map(|t| matches!(t, winit::window::Theme::Dark))
        });
        let source = match self.theme_mode {
            ThemeMode::Manual => return false,
            ThemeMode::FollowSystem => {
                OsThemeSource::follow_system(queries.follow_system_dark(hint))
            }
            // A desktop stating a light or dark preference decides; one
            // stating none (macOS, Windows) follows the winit hint.
            ThemeMode::Native => OsThemeSource::native((queries.native_desktop)(), hint),
        };
        self.apply_os_source(source)
    }

    /// Give every window, and the manager's own theme, the theme `source`
    /// resolves to at its density, except where that is the theme it was
    /// last given; whether any theme was set. Does not look at the theme
    /// mode: the caller has.
    pub(crate) fn apply_os_source(&mut self, source: OsThemeSource) -> bool {
        let mut update = OsThemeUpdate::new(self.native_theme.as_ref(), source);
        let mut changed = false;
        for managed in self.windows.values_mut() {
            if let Some(theme) =
                update.theme_for(&mut managed.os_theme, managed.tree.input_density())
            {
                // As in `set_theme`: the translator holds the input tokens.
                managed.translation_state.set_input_tokens(theme.input);
                managed.tree.set_theme(theme);
                changed = true;
            }
        }
        if let Some(theme) = update.theme_for(&mut self.os_theme, self.theme.input.density) {
            self.theme = theme;
            self.theme_generation += 1;
            changed = true;
        }
        // The application tree follows the application's theme, which has
        // no window density of its own to resolve against.
        self.sync_app_tree_theme();
        changed
    }

    /// The theme a window created now is built with, and, while the theme
    /// follows the OS, what it was resolved from. `window_hint` is the new
    /// window's winit light or dark state.
    ///
    /// Under `Manual` it is the manager's theme. Under `FollowSystem` it is
    /// the IntUI light or dark preset the desktop's preference names, and
    /// under `Native` the theme the desktop resolves to. Under either, a
    /// desktop that states no preference follows the hint, as
    /// [`apply_os_theme`](Self::apply_os_theme) does, so a window opened and
    /// a change heard later resolve alike. The theme is resolved at the
    /// density of the manager's theme and becomes
    /// the manager's theme, which later windows start from.
    fn new_window_theme(
        &mut self,
        window_hint: Option<bool>,
        queries: OsQueries,
    ) -> (Theme, Option<OsThemeInputs>) {
        let source = match self.theme_mode {
            ThemeMode::Manual => return (self.theme.clone(), None),
            ThemeMode::FollowSystem => {
                OsThemeSource::follow_system(queries.follow_system_dark(window_hint))
            }
            ThemeMode::Native => OsThemeSource::native((queries.native_desktop)(), window_hint),
        };
        let inputs = source.at(self.theme.input.density);
        let theme = resolve_os_theme(self.native_theme.as_ref(), &inputs);
        self.theme = theme.clone();
        self.theme_generation += 1;
        self.os_theme = Some(inputs.clone());
        // The application tree follows the application's theme, which this
        // has just resolved.
        self.sync_app_tree_theme();
        (theme, Some(inputs))
    }

    /// Install the shared typesetter every window's tree will shape text
    /// through (called by `TeksiloAppHandler` during initialization).
    pub fn set_typesetter(&mut self, typesetter: teksilo_text::SharedTypesetter) {
        self.typesetter = Some(typesetter);
    }

    /// Mint the next [`TeksiloWindowId`]. Monotone for the life of the manager
    /// and never reused, so a stale id names a closed window rather than a
    /// different live one.
    fn alloc_id(&mut self) -> TeksiloWindowId {
        let id = TeksiloWindowId::new(self.next_id);
        self.next_id += 1;
        id
    }

    /// Create a new window synchronously. Allocates an id, constructs
    /// the winit surface, builds the widget tree, and registers
    /// everything in the windows map before returning.
    ///
    /// The returned id is immediately usable — it can be passed to
    /// `find_window`, `focus_window`, or `close_window_by_id`, and
    /// state writes through `WindowState` are applied at the next
    /// `drain_window_commands` tick.
    pub fn create_window(
        &mut self,
        config: WindowConfig,
        target: &winit::event_loop::ActiveEventLoop,
    ) -> TeksiloWindowId {
        self.create_window_beside(config, target, None)
    }

    /// [`create_window`](Self::create_window) from a window's own handler,
    /// which runs with that window out of the map (`take_managed`).
    /// `dispatching` is that window, so a dialog `modal_to` it is still owned
    /// by it and centred over it.
    pub(crate) fn create_window_beside(
        &mut self,
        mut config: WindowConfig,
        target: &winit::event_loop::ActiveEventLoop,
        dispatching: Option<(TeksiloWindowId, &winit::window::Window)>,
    ) -> TeksiloWindowId {
        // If a `WindowStateService` is registered AND this window has
        // a stable `id(...)`, restore the saved geometry — sanitized
        // against the current monitor — into `config` before any
        // winit attribute is built. See `window_persist` for the
        // exact policy.
        let persist_service: Option<teksilo_settings::WindowStateService> =
            self.app_context_template.as_ref().and_then(|t| {
                t.app_state::<teksilo_settings::WindowStateService>()
                    .cloned()
            });
        // Restoring and persisting are separate decisions (see
        // `WindowConfig::restore_geometry`): a window can save its geometry
        // without reading the saved value back. That is what lets an app whose
        // windows share one geometry slot open the *first* window where the user
        // left it while letting the OS place any window opened alongside it —
        // instead of stacking them all on the same pixel — yet still have the
        // last-moved window be the one that reopens.
        if let Some(svc) = persist_service.as_ref()
            && config.restore_geometry
        {
            crate::window_persist::apply_restored_geometry(&mut config, svc, target);
        }

        let teksilo_id = self.alloc_id();
        // Drain the close-guard fields out of the config before any of
        // its other fields are consumed below; they move onto the
        // `ManagedWindow` and live for the window's lifetime.
        let close_guard = config.take_close_guard();
        let can_close = config.take_can_close();
        let on_close_blocked = config.take_close_blocked();
        let on_removed = config.take_on_removed();
        let state = WindowState::new(WindowStateInit {
            id: teksilo_id,
            string_id: config.string_id.clone(),
            placement: config.initial_placement,
            title: config.title.clone(),
            size: config.size,
            position: config.position.unwrap_or((0, 0)),
            focused: true,
            resizable: config.resizable,
            always_on_top: config.always_on_top,
        });
        // A title bar the app draws offers the buttons the OS one does.
        state.set_caption_buttons(config.is_minimizable(), config.is_maximizable());
        // A modal window has no minimise button whatever its config asks:
        // it follows the window it blocks.
        #[cfg(debug_assertions)]
        if config.is_modal() && config.minimizable == Some(true) {
            eprintln!(
                "teksilo-app: window `{}` is modal and asks for a minimise button; a modal \
                 window has none, it is minimised and restored with the window it blocks",
                config.title
            );
        }
        if let Some(sid) = &config.string_id {
            // Nothing refuses a second open window with the same id, and
            // `find_window` can name only one of them, so a debug build says
            // so.
            #[cfg(debug_assertions)]
            if let Some(open) = self.string_to_id.get(sid) {
                eprintln!(
                    "teksilo-app: a window with id `{sid}` is already open ({open:?}); \
                     `find_window(\"{sid}\")` now finds the new one. Look the id up \
                     before opening it to raise the open window instead"
                );
            }
            self.string_to_id.insert(sid.clone(), teksilo_id);
        }
        let wants_custom_chrome = config.decorations.wants_custom_chrome_host();
        let is_modal = config.is_modal();
        let modal_parent = config.modal_parent();
        let modal_focus_target = config.modal_focus_target();
        // A dialog comes to the front only for the user: asked in answer to
        // their input, or over a window that has the focus, itself or
        // through a dialog of it (the application is in front). Raising it
        // otherwise (macOS's `activateIgnoringOtherApps`, Windows'
        // `SetForegroundWindow`) would take the focus from the application
        // the user is in, and on macOS cancel the attention the application
        // asked for. Read before the activation token is used below.
        let raise_modal = is_modal
            && (std::mem::take(&mut self.raise_next_modal)
                || self.acts_for_user()
                || self.quit_awaits_answer()
                || modal_parent.is_some_and(|owner| self.knows_focused_with_dialogs(owner)));
        let owner_minimised = modal_parent.is_some_and(|owner| self.knows_minimised(owner));
        // A dialog not raised opens without taking the focus either: shown,
        // it would become the key window (macOS's `makeKeyAndOrderFront:`,
        // Windows' `SW_SHOW`) and take the keyboard from the window the user
        // is typing in, of this application or another.
        let activate = !is_modal || raise_modal;

        // Center modal windows over their parent when the caller did not
        // request a specific position: the dialog's frame on the parent's
        // frame. No-op on Wayland (compositor owns positioning).
        // The centre is kept for the size the dialog grows to (see
        // `ManagedWindow::autosize_centre`).
        let mut modal_centre = None;
        if config.position.is_none()
            && let Some(parent_window) = self.parent_window(modal_parent, dispatching)
            && let Ok(parent_outer_pos) = parent_window.outer_position()
        {
            let parent_sf = parent_window.scale_factor();
            let parent_outer_size = parent_window.outer_size();
            let p_x = parent_outer_pos.x as f64 / parent_sf;
            let p_y = parent_outer_pos.y as f64 / parent_sf;
            let p_w = parent_outer_size.width as f64 / parent_sf;
            let p_h = parent_outer_size.height as f64 / parent_sf;
            let centre = (p_x + p_w / 2.0, p_y + p_h / 2.0);
            // The dialog's frame is not there yet: its owner's stands for
            // it, the same window system drawing both. The size it grows to
            // is centred with its own (`autosize_centre`).
            config.position = Some(centred_on(centre, config.size, frame_insets(parent_window)));
            modal_centre = Some(centre);
        }

        let mut window_attrs = winit::window::Window::default_attributes()
            .with_title(&config.title)
            .with_inner_size(winit::dpi::LogicalSize::new(config.size.0, config.size.1))
            .with_resizable(config.resizable)
            .with_enabled_buttons(caption_buttons(&config))
            .with_visible(false) // Must be invisible for AccessKit adapter creation
            .with_active(activate);

        // The id the desktop matches this window against. On Wayland it is the
        // toplevel's `app_id`, and without it the shell cannot tie the window
        // to its desktop entry: it appears as an unnamed entry with the generic
        // icon, and `StartupWMClass` is not consulted, there being no
        // `WM_CLASS` on Wayland to put there. On X11 it is `WM_CLASS` itself.
        //
        // One call covers both: winit's Wayland and X11 extension traits write
        // the same `platform_specific.name` field (`platform/wayland.rs:113`
        // and `platform/x11.rs:212`), so this is the whole of it on every
        // display server the same build supports. Windows and macOS have no
        // equivalent and the field is simply unused there.
        #[cfg(any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        ))]
        if let Some(app_id) = config.app_id.as_deref() {
            use winit::platform::wayland::WindowAttributesExtWayland;
            window_attrs = window_attrs.with_name(app_id, "");
        }
        // On X11 a modal window is a dialog (`_NET_WM_WINDOW_TYPE_DIALOG`),
        // as GTK and Qt make theirs: with `WM_TRANSIENT_FOR` and
        // `_NET_WM_STATE_MODAL`, written below, that is what a window manager
        // reads to draw it with no minimise button and to keep it with its
        // parent. winit has no call for the buttons themselves there.
        // Ignored on Wayland.
        #[cfg(any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        ))]
        if is_modal {
            use winit::platform::x11::{WindowAttributesExtX11, WindowType};
            window_attrs = window_attrs.with_x11_window_type(vec![WindowType::Dialog]);
        }

        if let Some((min_w, min_h)) = config.min_size {
            window_attrs =
                window_attrs.with_min_inner_size(winit::dpi::LogicalSize::new(min_w, min_h));
        }
        if let Some((max_w, max_h)) = config.max_size {
            window_attrs =
                window_attrs.with_max_inner_size(winit::dpi::LogicalSize::new(max_w, max_h));
        }
        if let Some((x, y)) = config.position {
            window_attrs = window_attrs.with_position(winit::dpi::LogicalPosition::new(x, y));
        }
        if matches!(config.decorations, DecorationsMode::None) {
            window_attrs = window_attrs.with_decorations(false);
        }
        if let Some(icon) = &config.icon {
            if icon.is_valid() {
                match winit::window::Icon::from_rgba(icon.rgba.clone(), icon.width, icon.height) {
                    Ok(platform_icon) => {
                        window_attrs = window_attrs.with_window_icon(Some(platform_icon));
                    }
                    Err(e) => eprintln!(
                        "teksilo-app: failed to build window icon ({}×{}): {e}",
                        icon.width, icon.height
                    ),
                }
            } else {
                eprintln!(
                    "teksilo-app: window icon buffer size ({}) does not match {}×{}×4 ({}); \
                     dropping icon, window will open with platform default",
                    icon.rgba.len(),
                    icon.width,
                    icon.height,
                    icon.expected_len()
                );
            }
        }

        // When the application opts into custom chrome, suppress the
        // server-side decorations on platforms where they're entirely
        // client-drawn (Wayland, X11). On macOS the M3 recipe sets the
        // relevant attributes via `WindowAttributesExtMacOS` instead (below).
        //
        // On Windows the title-bar host makes the whole window the client
        // area (`WM_NCCALCSIZE`), so winit is told the window has no frame.
        // An undecorated top-level window keeps the caption and sizing styles
        // the host's recipe needs (aero snap, snap layouts, the DWM shadow);
        // what changes is winit's size arithmetic, which then stops adding a
        // frame the host has taken away. With the frame counted, the
        // configured size, a restored size, `request_inner_size` and the
        // minimum and maximum all came out larger by it (16 x 39 px at
        // 100 %), and a window restored from its saved size grew by that on
        // every launch. If the host cannot be installed,
        // `create_title_bar_host` gives the window its frame back.
        #[cfg(target_os = "windows")]
        if wants_custom_chrome {
            window_attrs = window_attrs.with_decorations(false);
        }
        //
        // The window system has to be predicted from the environment rather
        // than read off a handle, because this decision is made *before* the
        // window exists. `active_window_system` mirrors winit's own precedence
        // exactly so the two cannot disagree.
        //
        // X11 additionally requires a window manager that implements
        // `_NET_WM_MOVERESIZE`: without server-side decorations that is the
        // only way the window can be moved or resized, so shipping a
        // borderless window to a WM that lacks it would strand the user. The
        // probe is cached per process and `X11Host::new` consults the same
        // answer, so the decoration flag and the host can't diverge.
        #[cfg(all(unix, not(target_os = "macos")))]
        if wants_custom_chrome {
            let suppress_decorations = match teksilo_platform::active_window_system() {
                teksilo_platform::WindowSystem::Wayland => true,
                teksilo_platform::WindowSystem::X11 => {
                    teksilo_platform::x11::capabilities().supports_custom_chrome()
                }
                teksilo_platform::WindowSystem::Unknown => false,
            };
            if suppress_decorations {
                window_attrs = window_attrs.with_decorations(false);
            }
        }

        // macOS custom chrome: let the widget tree paint under the titlebar
        // region while keeping the native traffic-light cluster on top. See
        // `title_bar_host/macos.rs` for how the traffic-light inset is
        // measured and exposed through `reserved_leading_inset`.
        #[cfg(target_os = "macos")]
        if wants_custom_chrome {
            use winit::platform::macos::WindowAttributesExtMacOS;
            window_attrs = window_attrs
                .with_titlebar_transparent(true)
                .with_fullsize_content_view(true)
                .with_title_hidden(true);
        }

        // Z-order for modals comes from the parent relationship below
        // (`with_owner_window` on Win32; `with_parent_window` on X11/Wayland;
        // `attach_child_window` on macOS), not from `WindowLevel::AlwaysOnTop`.
        // Setting TOPMOST on a Win32 owned window is redundant and disrupts
        // the message pump (paint events stop arriving until the user forces
        // a redraw via focus change or resize). Only honour the explicit
        // `always_on_top` config flag here.
        if config.always_on_top {
            window_attrs = window_attrs.with_window_level(WindowLevel::AlwaysOnTop);
        }
        // Windows takes the taskbar flag at creation; X11 takes it between
        // creation and the first map, below.
        window_attrs =
            teksilo_platform::taskbar::skip_taskbar_attributes(window_attrs, config.skip_taskbar);

        // Parent-window attachment. Independent of the modal flag so
        // non-modal parented windows (popover-as-window, inspector
        // palettes, floating tool panels — the coming multi-window
        // cases) take the same path.
        //
        // Win32: use `with_owner_window` (CreateWindowEx's hwndOwner) —
        // winit documents this as "for dialog boxes". Produces an owned
        // WS_POPUP/WS_OVERLAPPED that floats above its owner, gets its
        // own paint/input messages, and tracks its owner's minimize
        // state. We do NOT use `with_parent_window` here: on Win32 winit
        // calls `SetParent`, making the dialog a `WS_CHILD` clipped
        // inside the owner's client area — wrong for dialogs in every
        // way (paint, input, movement, z-order).
        //
        // X11: `WM_TRANSIENT_FOR`, written before the first map
        // (`set_transient_for_before_show`, below). Not `with_parent_window`
        // either: on X11 winit creates the window inside its parent,
        // clipped to its client area. Wayland: nothing; winit ignores a
        // parent there and offers no `xdg_toplevel.set_parent`.
        //
        // macOS: skip here and defer to `attach_child_window` after
        // `PlatformWindow::new_with_a11y`. AppKit's
        // `-[NSWindow addChildWindow:ordered:]` orders the child
        // front (making it visible), which would race with the
        // AccessKit adapter that requires a hidden window at
        // construction.
        #[cfg(target_os = "windows")]
        if let Some(parent_window) = self.parent_window(modal_parent, dispatching)
            && let Ok(parent_handle) = parent_window.window_handle()
            && let winit::raw_window_handle::RawWindowHandle::Win32(win32) = parent_handle.as_raw()
        {
            use winit::platform::windows::WindowAttributesExtWindows;
            window_attrs = window_attrs.with_owner_window(win32.hwnd.get());
        }

        // The token the routed event carried, if its handler opens this
        // window (see `WithActivationToken`); otherwise, opted in, a startup
        // activation token from the environment so a window spawned by
        // another instance's "open in new window" comes up focused on
        // Wayland. No-op off Wayland/X11 or when the env is unset.
        if let Some(token) = self.use_dispatch_activation_token() {
            window_attrs = teksilo_platform::window_activation::apply_token(window_attrs, token);
        } else if config.activate_from_env {
            window_attrs =
                teksilo_platform::window_activation::apply_creation_token(window_attrs, target);
        }

        let window = target
            .create_window(window_attrs)
            .expect("winit window creation failed");
        // On macOS a position given at creation places the content's
        // top-left corner, where every other platform, `Moved`, the saved
        // geometry and the modal centring above mean the frame's: placed
        // so, a restored window opened a title bar higher each time. The
        // frame is moved there while the window is still hidden.
        #[cfg(target_os = "macos")]
        if let Some((x, y)) = config.position {
            window.set_outer_position(winit::dpi::LogicalPosition::new(x, y));
        }
        // Still unmapped: the window is shown once its accessibility adapter
        // exists, in `new_with_a11y` below.
        teksilo_platform::taskbar::skip_taskbar_before_show(&window, config.skip_taskbar);
        // A modal dialog and a window kept out of the taskbar are not listed
        // in the macOS Window and Dock menus: see `listed_in_window_menu`.
        teksilo_platform::set_listed_in_window_menu(&window, listed_in_window_menu(&config));
        if let Some(parent_window) = self.parent_window(modal_parent, dispatching) {
            teksilo_platform::set_transient_for_before_show(&window, parent_window);
            // And modal to the window manager (`_NET_WM_STATE_MODAL`), as GTK
            // marks its modal dialogs. Only with a parent: without
            // `WM_TRANSIENT_FOR` the state makes a window modal to every
            // window of the application.
            teksilo_platform::mark_modal_before_show(&window);
        }
        let winit_id = window.id();
        let scale_factor = window.scale_factor();

        let mut translation_state = TranslationState::new();
        translation_state.set_scale_factor(scale_factor);
        translation_state.set_pen_batching(self.pen_batching);

        // Resolve the initial theme from ThemeMode before building the tree.
        // OS-following modes carry the id "system" so a `ThemeSwitcher`
        // recognizes the active theme as "follow OS", not a fixed pick, and
        // update the shared theme so all subsequent windows use the same base.
        let window_hint = window
            .theme()
            .map(|theme| matches!(theme, winit::window::Theme::Dark));
        let (initial_theme, initial_os_theme) =
            self.new_window_theme(window_hint, OsQueries::PLATFORM);

        // Create with AccessKit adapter (shows window after adapter is ready)
        let mut pw = pollster::block_on(PlatformWindow::new_with_a11y(window, target, activate));
        #[cfg(test)]
        if !activate {
            activation_log::note(winit_id, activation_log::Activation::ShownInactive);
        }
        // Where the window system put it (macOS centres a window given no
        // position, Windows cascades it): a window never moved sends no
        // `Moved`, and its state is what its saved geometry records. Wayland
        // answers nothing, and the state keeps the config's position.
        if let Ok(outer) = pw.window().outer_position() {
            state.set_position_from_os((
                (outer.x as f64 / scale_factor).round() as i32,
                (outer.y as f64 / scale_factor).round() as i32,
            ));
        }
        // The routes the window's wake target posts through: state wakes
        // always (a redraw request does not reach a window whose redraw the
        // compositor withholds), draw wakes made off the main thread on macOS
        // (winit runs such a request on the main thread, synchronously). See
        // `TeksiloAppHandler::window_wake`.
        if let Some(proxy) = self.event_proxy.clone() {
            let (state, draw_off_main) = wake_routes(
                move |payload| proxy.send_external_boxed(payload),
                pw.window().id(),
                cfg!(target_os = "macos"),
            );
            pw.set_state_wake_route(state);
            if let Some(route) = draw_off_main {
                pw.set_off_main_wake_route(route);
            }
        }

        // Finish wiring the per-window input translator, now that a real
        // surface exists to ask about. What each of the three facts is for is
        // on `input_loop::wire_translator`; what is here is only the asking.
        //
        // `create_pen_source` never fails: it answers the null source — no
        // capabilities, so `wire_translator` installs nothing — wherever the
        // platform has no pen *path*. What it does NOT do is probe for an
        // attached digitizer, on any platform: a Wayland session whose
        // compositor advertises `zwp_tablet_manager_v2` and a Win32 window
        // whose subclass installs both get a live shim with no stylus in the
        // building. X11 and macOS have no path at all and always get the null
        // source.
        let window_system = winit::raw_window_handle::HasDisplayHandle::display_handle(pw.window())
            .ok()
            .map(|display| teksilo_platform::window_system_for_display_handle(&display.as_raw()));
        let pen = teksilo_core::raw_handle::ParentHandle::from_window(pw.window())
            .map(|parent| teksilo_platform::create_pen_source(&parent));
        crate::input_loop::wire_translator(
            &mut translation_state,
            window_system,
            initial_theme.input,
            pen,
        );

        // macOS-only: the parent-child attach was deferred out of the
        // winit builder above to avoid the AppKit auto-show that races
        // with AccessKit adapter creation. Wire it now that the child
        // is visible.
        //
        // Never onto a miniaturised parent: AppKit orders the parent in
        // without deminiaturising it, and draws both while the parent stays
        // in the Dock. The application tree's dialogs and a window's own
        // wait for the user in that case (`AppDialogPresentation::Owe`); a
        // window opened with `modal_to` over a minimised window gets it
        // unattached. AppKit is asked too, not only what the manager knows:
        // a window that took the focus on a Dock click, or that was just
        // restored for a dialog, still reports itself miniaturised until
        // its animation ends. The attach is owed then, and made on the
        // dialog's first sign once the parent is back
        // (`pay_owed_attach`).
        #[cfg(target_os = "macos")]
        {
            let parent_state = self
                .parent_window(modal_parent, dispatching)
                .map(|parent| parent.is_minimized() == Some(true));
            match (parent_state, modal_parent) {
                (Some(os_minimised), Some(owner)) if owner_minimised || os_minimised => {
                    #[cfg(debug_assertions)]
                    if owner_minimised {
                        eprintln!(
                            "teksilo-app: window `{}` is modal to a minimised window; it is \
                             attached to it once that window is back, as AppKit would draw \
                             the minimised window too",
                            config.title
                        );
                    }
                    self.attach_owed.insert(teksilo_id, owner);
                }
                (Some(_), _) => {
                    if let Some(parent_window) = self.parent_window(modal_parent, dispatching) {
                        teksilo_platform::attach_child_window(parent_window, pw.window());
                    }
                }
                (None, _) => {}
            }
        }
        #[cfg(not(target_os = "macos"))]
        let _ = owner_minimised;

        if raise_modal {
            // No `set_window_level(AlwaysOnTop)` here — see the comment
            // on `with_window_level` above. The owner relationship
            // already keeps the modal ordered above its parent.
            raise_window(pw.window(), None);
            // Hidden still (a macOS child window while the window it
            // blocks comes back from the Dock, an X11 window before its map
            // arrives), it did not take the raise: owe it, as
            // `raise_modal_child` does, and pay it on its first sign of
            // being on screen.
            if pw.window().is_visible() == Some(false) {
                self.raise_owed.insert(teksilo_id);
            }
        }

        // Construct the platform title bar host if custom chrome was
        // requested. When the window system cannot support it (X11 under a WM
        // without `_NET_WM_MOVERESIZE`, or a display handle that is neither
        // X11 nor Wayland) the factory returns `Unsupported`; we silently
        // continue with native decorations and leave the host slot empty.
        let title_bar_host: Option<Rc<dyn PlatformTitleBarHost>> = if wants_custom_chrome {
            let callbacks = match self.event_proxy.clone() {
                Some(proxy) => {
                    let close_proxy = proxy.clone();
                    let post_proxy = proxy;
                    TitleBarHostCallbacks {
                        request_close: Rc::new(move || {
                            close_proxy.send_external(CloseWindowRequest { teksilo_id });
                        }),
                        // Used by the Windows backend to post
                        // `TitleBarSyntheticEvent` / `TitleBarHoverEvent`
                        // back through `AppEvent::External`. Wayland
                        // and macOS construct the host but never call
                        // this closure.
                        post_external: Rc::new(move |payload| {
                            post_proxy.send_external_boxed(payload);
                        }),
                        teksilo_id,
                    }
                }
                // Headless / test path: no event loop proxy is installed, so
                // the host's close() becomes a silent no-op. Real windowed
                // runs always install a proxy via `set_event_proxy`.
                None => TitleBarHostCallbacks {
                    teksilo_id,
                    ..TitleBarHostCallbacks::noop()
                },
            };
            create_title_bar_host(pw.window_arc(), callbacks).ok()
        } else {
            None
        };

        let mut tree = self.new_window_tree(initial_theme);
        // Surface the window's HiDPI device scale to widgets that bridge to a
        // device-pixel OS resource (e.g. a `WebView` subview). Refreshed on
        // `ScaleFactorChanged`; the tree is otherwise fully logical.
        tree.set_device_scale_factor(scale_factor as f32);
        // Before the root builder runs, so a source attached in `build()`
        // already wakes this window.
        tree.set_redraw_waker(Some(pw.redraw_waker()));

        self.seed_tree(&mut tree);
        if let Some(ref host) = title_bar_host {
            tree.set_title_bar_host(host.clone());
        }

        let mut primed_atlas_version: u64 = 0;
        if let Some(ref typesetter) = self.typesetter {
            typesetter.set_scale_factor(scale_factor as f32);
            tree = tree.with_text_backend(typesetter.as_text_backend());

            // Prime the new window's GPU atlas from the shared
            // typesetter. The versioned path in
            // `handle_redraw_requested` only uploads when this
            // window's `atlas_uploaded_version` lags the bridge; a
            // window created after the atlas already contains every
            // glyph it needs (e.g. reopening a modal with the same
            // labels) would otherwise render text against an empty
            // per-window atlas texture. Read-only access on purpose:
            // calling `atlas_info` here would consume the pending
            // text-activity flag and the eviction-epoch delta that
            // belong to the creating window's in-flight redraw.
            let (w, h, pixels, version) = {
                let bridge = typesetter.bridge().borrow();
                let service = bridge.service();
                (
                    service.atlas_width(),
                    service.atlas_height(),
                    service.atlas_pixels().to_vec(),
                    bridge.atlas_version(),
                )
            };
            if w > 0 && h > 0 {
                pw.renderer_mut().upload_atlas(w, h, &pixels);
                primed_atlas_version = version;
            }
        }

        // Attach this window's state to the tree so widgets can bind
        // against its own window signals via `ctx.window()`.
        tree.set_window_state(state.clone());

        // The application menu's shortcuts, before the root builds, so a
        // menu row naming one shows its chord from the first frame.
        let mut app_shortcuts = crate::application_menu::MirroredShortcuts::default();
        self.sync_window_shortcuts(&mut app_shortcuts, &mut tree);

        if let Some(root_builder) = config.take_root_builder() {
            let mut root_id = root_builder(&mut tree, state.clone());

            // Apply post-root wrapping: per-window override takes
            // precedence; otherwise fall back to the app-wide
            // `DefaultPostRoot` registered via `app_state` (e.g. the
            // debug inspector's shell wrapper). The wrapped id becomes
            // the window's effective root for modal-focus lookup, since
            // the wrapper still descends into the user's tree.
            //
            // The app-wide default is intentionally skipped for modal
            // windows: it installs app-level chrome (the toast host, the
            // inspector shell) that should belong to the primary window,
            // not to transient native-window modals (dialogs, message
            // boxes, wizards). Without this guard the shared toast
            // registry would render every live toast in the modal too,
            // anchored to the wrong window. An app that genuinely wants
            // chrome on a specific modal can still set a per-window
            // override on that window's `WindowConfig`.
            let default_post_root = (!is_modal)
                .then(|| {
                    self.app_context_template
                        .as_ref()
                        .and_then(|t| t.app_state::<crate::DefaultPostRoot>().cloned())
                })
                .flatten();
            root_id = wrap_root(
                &mut tree,
                root_id,
                config.take_post_root_builder(),
                default_post_root,
            );

            if let Some(id) = initial_window_focus(&tree, root_id, is_modal, modal_focus_target) {
                tree.focus(id);
            }
        }

        // Apply non-placement post-creation tweaks that winit can't
        // express at builder time.
        if config.initial_placement.is_maximized() {
            pw.window().set_maximized(true);
        }
        if config.initial_placement.is_fullscreen() {
            pw.window()
                .set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
        }
        if config.initial_placement.is_minimized() {
            pw.window().set_minimized(true);
        }

        // Handle modal blocking
        if let Some(parent_id) = modal_parent {
            self.modal_blocked.insert(parent_id, teksilo_id);
            self.modal_parents.insert(teksilo_id, parent_id);
            let owner_minimised = self
                .get_by_teksilo_mut(parent_id)
                .is_some_and(|owner| owner.minimized);
            self.owners_seen.insert(
                teksilo_id,
                OwnerSeen {
                    minimised: owner_minimised,
                    went_with: false,
                },
            );
            // On macOS the bar shows the owner's menu while the dialog,
            // which has none, is key.
            if let Some(handle) = self
                .app_context_template
                .as_ref()
                .and_then(|t| t.app_state::<teksilo_platform::native_menu::NativeMenuHandle>())
            {
                handle.set_window_owner(teksilo_id, Some(parent_id));
            }
        }

        // Install the auto-save observers if persistence is wired and
        // this window opted in by carrying a stable id. The handles
        // outlive the function via `ManagedWindow._persist_handles`.
        let persist_handles = match (&persist_service, &config.string_id) {
            (Some(svc), Some(label)) => {
                crate::window_persist::install_persist_observers(&state, svc.clone(), label.clone())
            }
            _ => Vec::new(),
        };

        let managed = ManagedWindow {
            teksilo_id,
            string_id: config.string_id,
            tree,
            state,
            platform_window: pw,
            translation_state,
            current_modifiers: winit::keyboard::ModifiersState::empty(),
            modal: is_modal,
            parent: modal_parent,
            size_to_content: config.size_to_content,
            a11y_delivered_walk: 0,
            a11y_delivered_at: None,
            last_autosize: None,
            autosize_width_floor: None,
            autosize_asked_from: None,
            autosize_centre: modal_centre,
            title_bar_host,
            focused: true,
            occluded: false,
            minimized: config.initial_placement.is_minimized(),
            minimized_probed_at: None,
            occlusion_probed_at: None,
            session_away: self.session_away,
            caps_lock_active: false,
            os_theme: initial_os_theme,
            ime_allowed: None,
            ime_purpose: None,
            last_ime_preedit_empty: false,
            _persist_handles: persist_handles,
            close_guard,
            can_close,
            on_close_blocked,
            on_removed,
            modal_on_dismiss: None,
            owed_dialogs: Vec::new(),
            owed_dialogs_due: false,
            attention_asked: false,
            atlas_uploaded_version: primed_atlas_version,
            redraw: crate::redraw_gate::RedrawGate::default(),
            app_shortcuts,
        };

        let mut managed = managed;
        // The first safe-area read. It is refreshed on resize and on scale
        // change; this is the one that answers before the window has ever
        // been resized, which for a window created full-screen is the only
        // one that matters.
        {
            let chrome =
                crate::app::WindowChrome::new(&managed.platform_window, &managed.redraw, None);
            crate::input_loop::refresh_safe_area(&chrome, &mut managed.tree);
        }

        self.windows.insert(winit_id, managed);
        self.teksilo_to_winit.insert(teksilo_id, winit_id);
        // A window just opened is the one the user is about to work with,
        // whether or not the OS ever reports it focused.
        self.recency.touch(teksilo_id);
        // A dialog follows its owner: watch both on X11.
        if modal_parent.is_some() {
            self.sync_wm_state_watch();
        }

        // Register the window as an OS drop target if external drag-and-drop
        // was installed (no-op otherwise). Runs on the main thread, as macOS
        // requires for view manipulation.
        self.attach_external_dnd(teksilo_id, winit_id);

        teksilo_id
    }

    /// Register the just-created window as an OS drop target via the installed
    /// [`ExternalDndHandle`](teksilo_platform::external_dnd::ExternalDndHandle).
    /// No-op if the app did not call `install_external_dnd`, or if the window
    /// or poster handle can't be resolved.
    fn attach_external_dnd(&self, teksilo_id: TeksiloWindowId, winit_id: winit::window::WindowId) {
        use teksilo_platform::external_dnd::ExternalDndHandle;
        let Some(template) = self.app_context_template.as_ref() else {
            return;
        };
        let Some(handle) = template.app_state::<ExternalDndHandle>().cloned() else {
            return;
        };
        let Some(poster) = template.poster().cloned() else {
            return;
        };
        let Some(managed) = self.windows.get(&winit_id) else {
            return;
        };
        if let Some(parent) =
            teksilo_core::raw_handle::ParentHandle::from_window(managed.platform_window.window())
        {
            handle.attach(teksilo_id, parent, poster);
            // Seed the backend's scale factor. X11 needs it to report drop
            // positions in window-logical coordinates (its protocol is
            // physical-pixel only, with no per-window DPI to query); every
            // other backend ignores it. Kept current by the
            // `ScaleFactorChanged` arm in `app.rs`.
            handle.set_scale_factor(teksilo_id, managed.platform_window.window().scale_factor());
        }
    }

    /// Close a window by its TeksiloWindowId.
    ///
    /// The single choke point every close funnels through:
    /// [`process_pending`](Self::process_pending) calls this both for a
    /// forced close ([`queue_close`](Self::queue_close) /
    /// `close_window_by_id`) and for a guarded one
    /// ([`request_close`](Self::request_close)) once its guard has
    /// passed. That makes this the one place to fire
    /// [`WindowConfig::on_removed`](teksilo_core::WindowConfig::on_removed)
    /// so both paths are covered by a single call site instead of two.
    ///
    /// The window's modal dialogs close first, without asking their close
    /// guards: a dialog belongs to the window it blocks, and one left open
    /// would block nothing. A window the OS destroyed
    /// (`WindowEvent::Destroyed`, as Windows destroys an owned window with
    /// its owner) is closed here too, to forget it.
    pub fn close_window(&mut self, teksilo_id: TeksiloWindowId) {
        let dialogs: Vec<TeksiloWindowId> = self
            .modal_parents
            .iter()
            .filter(|(_, owner)| **owner == teksilo_id)
            .map(|(dialog, _)| *dialog)
            .collect();
        for dialog in dialogs {
            self.close_window(dialog);
        }
        // Purge any pending file-dialog callbacks owned by the
        // soon-to-close window before its tree is dropped — see
        // `teksilo_platform::file_dialog::FileDialogHandle::purge_window`.
        // A worker-thread future that resolves after this point will
        // still arrive at the dispatcher; deliver finds no pending
        // entry and silently drops.
        #[cfg(feature = "file-dialog")]
        {
            if let Some(handle) = self
                .app_context_template
                .as_ref()
                .and_then(|t| t.app_state::<teksilo_platform::file_dialog::FileDialogHandle>())
            {
                handle.purge_window(teksilo_id);
            }
        }
        // Purge any pending async completions owned by the closing window so a
        // late-arriving `spawn_local_with` result never touches a torn-down
        // tree (mirrors the file-dialog purge above; teksilo-core type, so no
        // feature gate).
        if let Some(handle) = self
            .app_context_template
            .as_ref()
            .and_then(|t| t.app_state::<teksilo_core::AsyncCompletionHandle>())
        {
            handle.purge_window(teksilo_id);
        }
        // Drop every subscription callback owned by the closing window: the
        // context-bearing ones (subscribe_event_with_ctx) and the plain ones
        // (subscribe_event) alike, so the shared TreeAppContext maps do not retain
        // inert closures per closed window. The window's tree is dropped wholesale
        // below, with no per-widget destroy pass, so these two calls are the only
        // thing that ever reaches those entries. A late backend event then finds
        // nothing.
        //
        // Neither purge may be the last owner of anything the `on_removed` hook at
        // the bottom of this function needs: that hook runs after the tree is gone,
        // and its callables are held separately by whoever registered them.
        if let Some(template) = self.app_context_template.as_ref() {
            template.purge_ctx_subscriptions_for_window(teksilo_id);
            template.purge_subscriptions_for_window(teksilo_id);
        }
        // Revoke the window's OS drop-target registration (drops the platform
        // guard — RevokeDragDrop / removeFromSuperview / data-device teardown).
        if let Some(handle) = self
            .app_context_template
            .as_ref()
            .and_then(|t| t.app_state::<teksilo_platform::external_dnd::ExternalDndHandle>())
        {
            handle.detach(teksilo_id);
        }
        // Forget this window's native (OS) menu + its activation map.
        if let Some(handle) = self
            .app_context_template
            .as_ref()
            .and_then(|t| t.app_state::<teksilo_platform::native_menu::NativeMenuHandle>())
        {
            handle.clear_window(teksilo_id);
        }
        // Drop any web-view event callbacks owned by this window so a late
        // backend event can't route into a torn-down tree.
        #[cfg(feature = "web-view")]
        if let Some(registry) = self
            .app_context_template
            .as_ref()
            .and_then(|t| t.app_state::<teksilo_webview::WebViewRegistry>())
        {
            registry.purge_window(teksilo_id);
        }
        // Stashed here (rather than invoked inline) so the hook can run
        // AFTER `managed` — and with it the tree, platform window, and
        // this whole `if let` block's own cleanup — is completely gone.
        // See `WindowConfig::on_removed` for why "after" is load-bearing.
        let mut removed_hook: Option<(teksilo_core::WindowRemovedCallback, Option<String>)> = None;
        self.recency.forget(teksilo_id);
        // A close it deferred no longer waits: a late answer changes nothing.
        self.deferred_closes.remove(&teksilo_id);
        if let Some(winit_id) = self.teksilo_to_winit.remove(&teksilo_id)
            && let Some(mut managed) = self.windows.remove(&winit_id)
        {
            // What it left unclaimed outlives it: see `orphan_intents`.
            let owners = self.owner_chain(teksilo_id);
            self.orphan_intents.extend(
                managed
                    .tree
                    .take_unclaimed_intents()
                    .into_iter()
                    .map(|intent| (intent, owners.clone())),
            );
            // If this window is involved in an in-flight app-originated OS
            // drag, abort it before the tree is dropped — otherwise the
            // app-global typed-payload stash would leak and a later genuine
            // external drop could be misrecovered as the stale payload.
            managed.tree.abort_outbound_drag();
            // Revoke every live contact at the translator before it is
            // dropped — see `input_loop::release_contacts` for what that
            // buys, which is not widget teardown.
            let now = managed.tree.input_now();
            crate::input_loop::release_contacts(&mut managed.translation_state, now);
            // The id goes with the window, unless another one opened with
            // it is still open: then the newest of those has it.
            if let Some(sid) = managed.string_id.as_deref() {
                match self
                    .windows
                    .values()
                    .filter(|other| other.string_id.as_deref() == Some(sid))
                    .map(|other| other.teksilo_id)
                    .max_by_key(|id| id.raw())
                {
                    Some(other) => {
                        self.string_to_id.insert(sid.to_owned(), other);
                    }
                    None => {
                        self.string_to_id.remove(sid);
                    }
                }
            }
            // Unblock the parent if this was the modal blocking it, and not
            // an earlier one a second modal has since replaced there.
            if managed.modal
                && let Some(parent_id) = managed.parent
                && self.modal_blocked.get(&parent_id) == Some(&teksilo_id)
            {
                self.modal_blocked.remove(&parent_id);
            }
            if let Some(hook) = managed.on_removed.take() {
                removed_hook = Some((hook, managed.string_id.clone()));
            }
        }
        // Also remove any modal children blocking this window
        self.modal_blocked.remove(&teksilo_id);
        let was_in_a_pair = self.modal_parents.remove(&teksilo_id).is_some();
        self.owners_seen.remove(&teksilo_id);
        self.raise_owed.remove(&teksilo_id);
        self.attach_owed.remove(&teksilo_id);
        self.modal_hosts.remove(&teksilo_id);
        if was_in_a_pair {
            self.sync_wm_state_watch();
        }

        // Fire the teardown hook last, once every registry above
        // (`windows`, `teksilo_to_winit`, `string_to_id`, `modal_blocked`)
        // no longer mentions this window — `remaining_windows` below is
        // `self.windows.len()` read at this point, so it already excludes
        // the window being removed.
        if let Some((hook, string_id)) = removed_hook {
            hook(&teksilo_core::WindowRemovedEvent {
                id: teksilo_id,
                string_id,
                remaining_windows: self.windows.len(),
            });
        }
    }

    /// Close `teksilo_id` as [`close_window`](Self::close_window) does,
    /// after answering the dialogs still up in it and over it through their
    /// own dismissal: the native modal windows it owns, deepest first, run
    /// their request's `on_dismiss`, and every overlay in their trees and
    /// its own is dismissed with its callbacks, all with
    /// [`DismissReason::Programmatic`](teksilo_core::overlay::DismissReason).
    /// So a `MessageBox` still up when a quit or a forced close of its
    /// window takes it reports its escape button (Cancel), as Escape would,
    /// rather than going unanswered, and so does one asked and not shown
    /// yet (owed to the user, or queued in this turn by the window's close
    /// guard). A dialog window closing by itself runs
    /// its own `on_dismiss` the same way, which a dialog that already
    /// answered ignores.
    pub(crate) fn close_window_answering(
        &mut self,
        teksilo_id: TeksiloWindowId,
        target: &winit::event_loop::ActiveEventLoop,
    ) {
        let mut chain = self.modal_descendants(teksilo_id);
        chain.push(teksilo_id);
        for window in chain {
            self.answer_dialogs_in(window, target);
        }
        self.close_window(teksilo_id);
    }

    /// The native modal windows `teksilo_id` owns, at any depth, deepest
    /// first.
    fn modal_descendants(&self, teksilo_id: TeksiloWindowId) -> Vec<TeksiloWindowId> {
        let mut found: Vec<TeksiloWindowId> = Vec::new();
        let mut frontier = vec![teksilo_id];
        // A modal chain is short and acyclic; the bound only keeps a
        // corrupt map from hanging the loop.
        for _ in 0..64 {
            let next: Vec<TeksiloWindowId> = self
                .modal_parents
                .iter()
                .filter(|(child, parent)| frontier.contains(parent) && !found.contains(child))
                .map(|(child, _)| *child)
                .collect();
            if next.is_empty() {
                break;
            }
            found.extend(next.iter().copied());
            frontier = next;
        }
        found.reverse();
        found
    }

    /// Answer the dialogs `teksilo_id` presents: the dialogs it owes the
    /// user ([`owe_window_dialog`](Self::owe_window_dialog)) and those its
    /// tree asked for and has not presented yet, its modal
    /// request's `on_dismiss` if it is a dialog window, and every overlay in
    /// its tree, with its own context. See
    /// [`close_window_answering`](Self::close_window_answering).
    fn answer_dialogs_in(
        &mut self,
        teksilo_id: TeksiloWindowId,
        target: &winit::event_loop::ActiveEventLoop,
    ) {
        let Some(&winit_id) = self.teksilo_to_winit.get(&teksilo_id) else {
            return;
        };
        let Some(mut managed) = self.take_managed(winit_id) else {
            return;
        };
        let on_dismiss = managed.modal_on_dismiss.take();
        let owed = std::mem::take(&mut managed.owed_dialogs);
        // And those asked and not presented yet: a close guard's question
        // queued in the turn its window closes (a quit with no time to ask
        // closes it at once).
        let queued = managed.tree.drain_pending_modal_requests();
        let current_arc = Some(managed.platform_window.window_arc());
        {
            let mut ops = WindowOpsImpl::new(self, target, teksilo_id, current_arc);
            let reason = teksilo_core::overlay::DismissReason::Programmatic;
            // The dialogs it still owes the user, never shown, first: they
            // would have been in front of everything else.
            for request in owed
                .into_iter()
                .chain(queued.into_iter().map(|queued| queued.request))
            {
                answer_unshown_dialog(request, &mut managed.tree, &mut ops);
            }
            managed.tree.dismiss_all_overlays_with_ops(reason, &mut ops);
            if let Some(on_dismiss) = on_dismiss {
                managed
                    .tree
                    .run_with_event_context(&mut ops, |ctx| on_dismiss(reason, ctx));
            }
        }
        self.reinsert_managed(winit_id, managed);
    }

    /// Queue an **unconditional** window closure (processed in the next
    /// event loop tick). The window's close guard is *not* consulted —
    /// use this for explicit programmatic closes and framework-internal
    /// teardown (modal dismissals, `close_window_by_id`). For a
    /// *guarded* close that a window's
    /// [`on_close_requested`](teksilo_core::WindowConfig::on_close_requested)
    /// can veto, use [`request_close`](Self::request_close).
    pub fn queue_close(&mut self, teksilo_id: TeksiloWindowId) {
        self.pending_closes.push(PendingClose {
            id: teksilo_id,
            force: true,
        });
    }

    /// Queue a **guarded** window closure (processed in the next event
    /// loop tick). Before tearing the window down,
    /// [`process_pending`](Self::process_pending) consults the window's
    /// close guard (from
    /// [`WindowConfig::on_close_requested`](teksilo_core::WindowConfig::on_close_requested)
    /// / [`can_close`](teksilo_core::WindowConfig::can_close)); a
    /// [`CloseResponse::Veto`](teksilo_core::CloseResponse) keeps the
    /// window open and a
    /// [`CloseResponse::Defer`] keeps it
    /// open until its answer. Used for the interactive close gestures: the OS
    /// close button, a custom-chrome close button, and
    /// [`EventContext::close_window`](teksilo_core::widget::EventContext::close_window).
    ///
    /// A window a modal dialog blocks refuses it: the dialog is brought
    /// forward and nothing closes, as a window with a sheet on macOS and a
    /// disabled owner on Windows refuse their close button. The dialog is
    /// what the user answers first. A dialog already queued to close lets
    /// the request through.
    pub fn request_close(&mut self, teksilo_id: TeksiloWindowId) {
        if self.blocking_modal_child(teksilo_id).is_some() {
            self.refocus_modal_child(teksilo_id);
            return;
        }
        self.pending_closes.push(PendingClose {
            id: teksilo_id,
            force: false,
        });
    }

    /// Route a Windows-side synthetic title-bar tap. The wndproc
    /// posts a `TitleBarSyntheticEvent` when `WM_NCLBUTTONUP`
    /// fires on a button rect that the OS treated as non-client; we
    /// resolve the matching `WidgetId` via the host and synthesise a
    /// primary-button tap so the widget's normal `on_tap` handler
    /// runs. No-op on platforms that never produce these events.
    pub fn route_title_bar_synthetic_tap(
        &mut self,
        teksilo_id: TeksiloWindowId,
        target: teksilo_core::ControlTarget,
    ) {
        let Some(winit_id) = self.teksilo_to_winit.get(&teksilo_id).copied() else {
            return;
        };
        let Some(managed) = self.windows.get_mut(&winit_id) else {
            return;
        };
        let Some(host) = managed.title_bar_host.as_ref() else {
            return;
        };
        let Some(button_id) = host.title_bar_widget_id(target) else {
            return;
        };
        managed.tree.synthesise_tap(button_id);
    }

    /// Route a Windows-side synthetic title-bar hover entered/leave.
    /// Delegates to the host's `set_button_hover`, which writes the
    /// signal `WindowControls` registered for the matching button.
    /// No-op on platforms that don't intercept non-client hover.
    pub fn route_title_bar_synthetic_hover(
        &mut self,
        teksilo_id: TeksiloWindowId,
        target: teksilo_core::ControlTarget,
        entered: bool,
    ) {
        let Some(winit_id) = self.teksilo_to_winit.get(&teksilo_id).copied() else {
            return;
        };
        let Some(managed) = self.windows.get(&winit_id) else {
            return;
        };
        let Some(host) = managed.title_bar_host.as_ref() else {
            return;
        };
        host.set_button_hover(target, entered);
    }

    /// Drain the app→OS command queue on every window and translate
    /// each [`WindowCommand`] into the appropriate winit call. Called
    /// once per event-loop tick after event dispatch.
    ///
    /// Observers on [`WindowState`] signals emit commands when app
    /// code writes through them. OS-originated writes go through the
    /// `*_from_os` setters on the state, which flip the re-entrancy
    /// guard so the same observers do not fire an echo back out — so
    /// the queue only contains genuine app→OS directives.
    pub fn drain_window_commands(&mut self) {
        // Collect (winit_id, cmd) pairs first so the borrow on
        // `self.windows` is released before we touch platform_window.
        let mut batch: Vec<(winit::window::WindowId, TeksiloWindowId, WindowCommand)> = Vec::new();
        for (winit_id, managed) in self.windows.iter() {
            for cmd in managed.state.drain_os_commands() {
                batch.push((*winit_id, managed.teksilo_id, cmd));
            }
        }
        for (winit_id, teksilo_id, cmd) in batch {
            // `Close` is the one command that needs to mutate
            // `self.windows` — queue it for the tick-end close drain
            // instead of running it inline. `WindowState::close()` is an
            // explicit programmatic close, so it bypasses the close
            // guard (forced); interactive gestures go through
            // `request_close` instead.
            if matches!(cmd, WindowCommand::Close) {
                self.queue_close(teksilo_id);
                continue;
            }
            // A focus with no token of its own takes the routed event's.
            let cmd = match cmd {
                WindowCommand::Focus {
                    activation_token: None,
                } => WindowCommand::Focus {
                    activation_token: self.focus_command_token.take(),
                },
                cmd => cmd,
            };
            let Some(managed) = self.windows.get_mut(&winit_id) else {
                continue;
            };
            if let Some(size) = apply_window_command(managed.platform_window.window(), cmd) {
                managed.took_own_resize(size);
            }
        }
    }

    /// Process pending window closures. Called from the event loop
    /// each tick. Creation does not need a drain path: `open_window`
    /// from handler code goes through [`WindowOpsImpl`] and calls
    /// [`create_window`](Self::create_window) synchronously inside the
    /// same dispatch.
    ///
    /// The closes guards deferred are settled first: one whose decision
    /// proceeded closes here, and a quit waiting on one cancelled is
    /// abandoned. Then a *forced* close
    /// ([`queue_close`](Self::queue_close)) tears the window down
    /// immediately, and a *guarded* one
    /// ([`request_close`](Self::request_close)) first runs the window's
    /// close guard via `evaluate_close_guard`: `Close` closes, `Veto` keeps
    /// the window open, and `Defer` keeps it open until its decision is
    /// answered. A guarded close of a window whose earlier close is still
    /// deferred asks nothing and brings the window forward: the user is
    /// already being asked. Guards are strictly per-window, so this is
    /// correct for multi-window apps: each pending close consults only its
    /// own window's guard.
    pub fn process_pending(&mut self, target: &winit::event_loop::ActiveEventLoop) {
        self.settle_deferred_closes();
        self.process_closes(target);
    }

    /// Carry out the closes queued so far; see
    /// [`process_pending`](Self::process_pending).
    fn process_closes(&mut self, target: &winit::event_loop::ActiveEventLoop) {
        let closes = std::mem::take(&mut self.pending_closes);
        for pending in closes {
            if pending.force {
                self.close_window_answering(pending.id, target);
                continue;
            }
            // A guarded close of a window its dialog still blocks (the
            // dialog's own close was vetoed this turn) is refused, as
            // `request_close` refuses it.
            if self.blocking_modal_child(pending.id).is_some() {
                self.refocus_modal_child(pending.id);
                continue;
            }
            if self.deferred_closes.contains_key(&pending.id) {
                let front = self.frontmost_modal_of(pending.id);
                self.bring_forward(front, None);
                continue;
            }
            match self.evaluate_close_guard(pending.id, target) {
                CloseResponse::Close => self.close_window_answering(pending.id, target),
                CloseResponse::Veto => {}
                CloseResponse::Defer(decision) => self.defer_close(pending.id, &decision),
            }
        }
    }

    /// Record that `teksilo_id`'s guard deferred its close with `decision`,
    /// and have its answer wake the event loop, from whatever thread gives
    /// it, so the turn that follows settles it.
    fn defer_close(&mut self, teksilo_id: TeksiloWindowId, decision: &teksilo_core::CloseDecision) {
        let receipt = self.wake_on_answer(decision);
        self.deferred_closes.insert(teksilo_id, receipt);
    }

    /// `decision`'s receipt, set to wake the event loop when it is
    /// answered, from whatever thread answers it, so the turn that follows
    /// settles it.
    fn wake_on_answer(
        &self,
        decision: &teksilo_core::CloseDecision,
    ) -> teksilo_core::window::CloseDecisionReceipt {
        let receipt = decision.receipt();
        if let Some(proxy) = self.event_proxy.clone() {
            let wake: std::sync::Arc<dyn Fn() + Send + Sync> = std::sync::Arc::new(move || {
                proxy.send_external_boxed(Box::new(crate::app::CloseAnswered));
            });
            receipt.on_answer(wake.clone());
            // Answered already (by the guard itself, or by a thread before
            // the wake was set): no answer is coming to wake the loop, and
            // the turn may be the last before it sleeps.
            if receipt.answer().is_some() {
                wake();
            }
        }
        receipt
    }

    /// Act on the answers given to deferred closes: a window whose decision
    /// proceeded is queued to close, without its guard (it gave its
    /// answer), and a quit waiting on it goes on; one cancelled, or whose
    /// decision was dropped unanswered, stays open, and a quit waiting on
    /// it is abandoned. The application's answer to a quit is settled the
    /// same way: proceed and the quit goes on to the windows, cancel and it
    /// is abandoned.
    fn settle_deferred_closes(&mut self) {
        use crate::quit::Awaited;
        if let Some(answer) = self
            .app_quit_deferral
            .as_ref()
            .and_then(|receipt| receipt.answer())
        {
            self.app_quit_deferral = None;
            let awaited = self
                .quit
                .is_some_and(|quit| quit.awaiting == Some(Awaited::Application));
            if awaited {
                match answer {
                    teksilo_core::window::CloseAnswer::Proceed => {
                        if let Some(quit) = &mut self.quit {
                            quit.awaiting = None;
                            quit.application_agreed = true;
                        }
                    }
                    teksilo_core::window::CloseAnswer::Cancel => self.end_quit(),
                    teksilo_core::window::CloseAnswer::Dropped => {
                        #[cfg(debug_assertions)]
                        eprintln!(
                            "teksilo-app: the application's quit guard deferred its answer and \
                             dropped the CloseDecision without answering; the quit is \
                             cancelled. Call proceed() or cancel() on it."
                        );
                        self.end_quit();
                    }
                }
            }
        }
        let answered: Vec<(TeksiloWindowId, teksilo_core::window::CloseAnswer)> = self
            .deferred_closes
            .iter()
            .filter_map(|(id, receipt)| receipt.answer().map(|answer| (*id, answer)))
            .collect();
        for (id, answer) in answered {
            self.deferred_closes.remove(&id);
            let awaited = self
                .quit
                .is_some_and(|quit| quit.awaiting == Some(Awaited::Window(id)));
            match answer {
                teksilo_core::window::CloseAnswer::Proceed => {
                    if awaited && let Some(quit) = &mut self.quit {
                        quit.awaiting = None;
                    }
                    self.queue_close(id);
                }
                teksilo_core::window::CloseAnswer::Cancel => {
                    if awaited {
                        self.end_quit();
                    }
                }
                teksilo_core::window::CloseAnswer::Dropped => {
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "teksilo-app: the close guard of window {} deferred its answer and \
                         dropped the CloseDecision without answering; the close is cancelled. \
                         Call proceed() or cancel() on it.",
                        id.raw()
                    );
                    if awaited {
                        self.end_quit();
                    }
                }
            }
        }
    }

    /// Ask the application to quit through the close guards; see
    /// [`EventContext::quit`](teksilo_core::widget::EventContext::quit).
    /// Changes nothing while a quit is under way or once one has completed.
    /// Carried out by
    /// [`advance_quit`](Self::advance_quit).
    pub fn quit(&mut self) {
        self.request_quit_from(
            crate::quit::QuitSource::Application,
            crate::quit::QuitMode::Guarded,
            None,
        );
    }

    /// Quit without asking any close guard; see
    /// [`EventContext::quit_forced`](teksilo_core::widget::EventContext::quit_forced).
    /// Makes a quit under way forced.
    pub fn quit_forced(&mut self) {
        self.request_quit_from(
            crate::quit::QuitSource::Application,
            crate::quit::QuitMode::Forced,
            None,
        );
    }

    /// Ask the application to quit on behalf of `source`, treating the
    /// guards (the application's, then every window's) as `mode` says;
    /// `reply` is told how the quit ends. The one entry of every quit
    /// source; see [`crate::quit`]. A request while a quit is under way
    /// joins it, and one in a stronger mode makes it that mode, read by
    /// the guards as coming from `source`; either way its `reply` hears how
    /// that quit ends. Once a quit has completed, a request starts nothing
    /// and its `reply` hears [`Completed`](crate::quit::QuitOutcome::Completed)
    /// at once. Carried out by [`advance_quit`](Self::advance_quit)
    /// after the next [`process_pending`](Self::process_pending).
    pub(crate) fn request_quit_from(
        &mut self,
        source: crate::quit::QuitSource,
        mode: crate::quit::QuitMode,
        reply: Option<crate::quit::QuitReply>,
    ) {
        // The application is ending: a request after its quit completed
        // (a SIGHUP on the heels of a SIGTERM) asks nobody again.
        if self.quit_completed {
            if let Some(reply) = reply {
                reply(crate::quit::QuitOutcome::Completed);
            }
            return;
        }
        self.quit_replies.extend(reply);
        match &mut self.quit {
            Some(quit) if mode > quit.mode => {
                // What the quit waited on is not awaited any more: an
                // overruling quit asks again, a forced one asks nobody.
                self.quit_generation += 1;
                quit.mode = mode;
                quit.source = source;
                quit.awaiting = None;
                quit.generation = self.quit_generation;
                self.app_quit_deferral = None;
            }
            Some(_) => {}
            None => {
                self.quit_generation += 1;
                self.quit_bound_reported = false;
                self.quit = Some(crate::quit::PendingQuit {
                    mode,
                    application_agreed: false,
                    awaiting: None,
                    source,
                    generation: self.quit_generation,
                });
            }
        }
    }

    /// The quit under way ends without completing: abandoned by a guard, or
    /// cancelled. An answer the application's guard still owes it is
    /// forgotten.
    fn end_quit(&mut self) {
        self.quit = None;
        self.app_quit_deferral = None;
        self.reply_to_quit(crate::quit::QuitOutcome::Cancelled);
    }

    /// Tell every reply of the quit that just ended how it ended.
    fn reply_to_quit(&mut self, outcome: crate::quit::QuitOutcome) {
        for reply in std::mem::take(&mut self.quit_replies) {
            reply(outcome);
        }
    }

    /// The source of the quit under way, if any.
    pub(crate) fn quit_source(&self) -> Option<crate::quit::QuitSource> {
        self.quit.map(|quit| quit.source)
    }

    /// Install the application's quit guard; see
    /// [`TeksiloAppBuilder::on_quit_requested`](crate::TeksiloAppBuilder::on_quit_requested).
    pub(crate) fn set_app_quit_guard(&mut self, guard: Option<CloseGuard>) {
        self.app_quit_guard = guard;
    }

    /// Ask the application's quit guard, in the application tree, and
    /// return its answer: `Close` with no guard or no application tree.
    fn evaluate_app_quit_guard(
        &mut self,
        target: &winit::event_loop::ActiveEventLoop,
    ) -> CloseResponse {
        let Some(guard) = self.app_quit_guard.clone() else {
            return CloseResponse::Close;
        };
        let Some(mut tree) = self.take_app_tree() else {
            return CloseResponse::Close;
        };
        let mut response = CloseResponse::Close;
        {
            let mut ops = WindowOpsImpl::windowless(self, target);
            tree.run_with_event_context(&mut ops, |ctx| response = guard(ctx));
        }
        self.reinsert_app_tree(tree);
        response
    }

    /// Whether a quit is under way: requested, and neither completed nor
    /// abandoned.
    pub fn quit_in_progress(&self) -> bool {
        self.quit.is_some()
    }

    /// Abandon the quit under way if it is guarded; see
    /// [`EventContext::cancel_quit`](teksilo_core::widget::EventContext::cancel_quit).
    /// An overruling or forced quit goes on: a guard or a dialog's Cancel
    /// arm run as its window closes cannot stop what no guard may refuse.
    pub fn cancel_quit(&mut self) {
        match self.quit {
            Some(quit) if quit.mode == crate::quit::QuitMode::Guarded => self.end_quit(),
            Some(_quit) => {
                #[cfg(debug_assertions)]
                eprintln!(
                    "teksilo-app: cancel_quit() during a quit no guard may refuse \
                     ({:?}, {:?}); the quit goes on",
                    _quit.source, _quit.mode
                );
            }
            None => {}
        }
    }

    /// Whether a quit ran to completion, closing every window. The event
    /// loop ends then, whatever the application's
    /// [`QuitPolicy`](crate::QuitPolicy).
    pub fn quit_completed(&self) -> bool {
        self.quit_completed
    }

    /// Forget that a quit completed, as if the application had just
    /// started: the loop test goes on quitting after its loop was told to
    /// end, which an application never does.
    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn forget_completed_quit(&mut self) {
        self.quit_completed = false;
    }

    /// The window `teksilo_id` is a modal child of, if any. Read from
    /// `modal_parents` rather than from the window, which a handler may
    /// have taken out of the map while it runs.
    pub(crate) fn modal_parent_of(&self, teksilo_id: TeksiloWindowId) -> Option<TeksiloWindowId> {
        self.modal_parents.get(&teksilo_id).copied()
    }

    /// The window to bring forward for `teksilo_id`'s dialog: its deepest
    /// open native modal descendant, or itself.
    pub(crate) fn frontmost_modal_of(&self, teksilo_id: TeksiloWindowId) -> TeksiloWindowId {
        let mut front = teksilo_id;
        for _ in 0..64 {
            match self
                .modal_parents
                .iter()
                .filter(|(child, parent)| **parent == front && !self.is_closing(**child))
                .map(|(child, _)| *child)
                .max_by_key(|child| self.recency.stamp(*child))
            {
                Some(child) => front = child,
                None => break,
            }
        }
        front
    }

    /// Whether `teksilo_id` has a close queued for this turn's
    /// [`process_pending`](Self::process_pending).
    pub(crate) fn is_closing(&self, teksilo_id: TeksiloWindowId) -> bool {
        self.pending_closes
            .iter()
            .any(|pending| pending.id == teksilo_id)
    }

    /// The window the application tree's dialogs and intents go to: the open
    /// window used last of those the user can see (not minimised, as far as
    /// the manager knows), else the open window used last, at its frontmost
    /// native dialog. A window about to close is passed over. `None` when no
    /// window is open or every one is closing. A dialog presented there
    /// first [brings it to the user](Self::show_for_app_tree_dialog).
    pub(crate) fn app_modal_target(&self) -> Option<TeksiloWindowId> {
        let open = |id: TeksiloWindowId| {
            self.teksilo_to_winit
                .get(&id)
                .and_then(|winit| self.windows.get(winit))
                .filter(|_| !self.is_closing(id))
        };
        let window = self
            .recency
            .most_recent_where(|id| open(id).is_some_and(|managed| !managed.minimized))
            .or_else(|| self.recency.most_recent_where(|id| open(id).is_some()))?;
        Some(self.frontmost_modal_of(window))
    }

    /// Make `teksilo_id` seen: restore it and every window up its chain of
    /// modal owners if minimised (a dialog is shown only with its owner),
    /// then raise it, activated with `token` where the window system needs
    /// one.
    pub(crate) fn bring_forward(&mut self, teksilo_id: TeksiloWindowId, token: Option<&str>) {
        self.restore_with_owners(teksilo_id);
        if let Some(managed) = self.get_by_teksilo_mut(teksilo_id) {
            raise_window(managed.platform_window.window(), token);
        }
    }

    /// Restore `teksilo_id` and every window up its chain of modal owners
    /// if minimised (a dialog is shown only with its owner), asking the
    /// window system as well as what the manager knows
    /// ([`ManagedWindow::restore`]), and raise nothing: the window a dialog
    /// is about to open over, for the user, comes back first.
    pub(crate) fn restore_with_owners(&mut self, teksilo_id: TeksiloWindowId) {
        let mut owner = self.modal_parent_of(teksilo_id);
        for _ in 0..64 {
            let Some(up) = owner else { break };
            if let Some(managed) = self.get_by_teksilo_mut(up) {
                managed.restore();
            }
            owner = self.modal_parent_of(up);
        }
        if let Some(managed) = self.get_by_teksilo_mut(teksilo_id) {
            managed.restore();
        }
    }

    /// The application tree's dialog is about to be presented in `window`
    /// (see [`app_modal_target`](Self::app_modal_target)) as `presentation`
    /// says ([`app_dialog_presentation`]): bring it to the user. Raised, it
    /// is [brought forward](Self::bring_forward), restored if minimised,
    /// with the turn's activation token, unless it has the focus already.
    /// Otherwise it asks for the user's attention (a taskbar button
    /// flashing, the Dock icon bouncing) and stays where it is, as an
    /// application in the background does: a timer's or another thread's
    /// dialog does not take the focus from the application the user is in.
    pub(crate) fn show_for_app_tree_dialog(
        &mut self,
        window: TeksiloWindowId,
        presentation: AppDialogPresentation,
    ) {
        let Some(managed) = self.get_by_teksilo_mut(window) else {
            return;
        };
        match presentation {
            AppDialogPresentation::Raise => {
                if managed.focused {
                    return;
                }
                let token = self
                    .use_dispatch_activation_token()
                    .or_else(|| self.focus_command_token.clone());
                self.bring_forward(window, token.as_deref());
            }
            AppDialogPresentation::Quiet | AppDialogPresentation::Owe => {
                managed.ask_attention();
            }
        }
    }

    /// A dialog is about to open over `window` without taking the focus
    /// ([`AppDialogPresentation::Quiet`]): `window` asks for the user's
    /// attention, as [`show_for_app_tree_dialog`](Self::show_for_app_tree_dialog)
    /// does for the application tree's. For a window's own dialog, in its
    /// tree or in a native window of its own, as AppKit asks for a modal
    /// panel of an application in the background. Nothing when `window` is
    /// not open.
    pub(crate) fn ask_attention_for_dialog(&mut self, window: TeksiloWindowId) {
        if let Some(managed) = self.get_by_teksilo_mut(window) {
            managed.ask_attention();
        }
    }

    /// What the manager knows of `teksilo_id`: whether it has the focus and
    /// whether it is minimised, from the map, or, for a window a handler
    /// runs in, as it was when taken out. `None` for no open window.
    fn known_state(&self, teksilo_id: TeksiloWindowId) -> Option<TakenOut> {
        if let Some(taken) = self.taken_out.get(&teksilo_id) {
            return Some(*taken);
        }
        let managed = self.windows.get(self.teksilo_to_winit.get(&teksilo_id)?)?;
        Some(TakenOut {
            focused: managed.focused,
            minimized: managed.minimized,
        })
    }

    /// Whether `teksilo_id` has the focus, as far as the manager knows (see
    /// [`known_state`](Self::known_state)).
    pub(crate) fn knows_focused(&self, teksilo_id: TeksiloWindowId) -> bool {
        self.known_state(teksilo_id)
            .is_some_and(|known| known.focused)
    }

    /// Whether `teksilo_id` or a dialog of it has the focus, as far as the
    /// manager knows (see [`knows_focused`](Self::knows_focused)): the user
    /// is with `teksilo_id`, as [`note_owed_dialogs_focus`](Self::note_owed_dialogs_focus)
    /// counts it. A dialog asked over it is raised and asks for no
    /// attention ([`app_dialog_presentation`]).
    pub(crate) fn knows_focused_with_dialogs(&self, teksilo_id: TeksiloWindowId) -> bool {
        self.teksilo_to_winit
            .keys()
            .chain(self.taken_out.keys())
            .any(|&id| {
                self.knows_focused(id)
                    && (id == teksilo_id || self.owner_chain(id).contains(&teksilo_id))
            })
    }

    /// Whether `teksilo_id` is minimised, as far as the manager knows (see
    /// [`known_state`](Self::known_state)).
    pub(crate) fn knows_minimised(&self, teksilo_id: TeksiloWindowId) -> bool {
        self.known_state(teksilo_id)
            .is_some_and(|known| known.minimized)
    }

    /// Re-read every unfocused window's minimised state, at most every
    /// [`MINIMIZED_PROBE_INTERVAL`] (see
    /// [`ManagedWindow::refresh_minimized`]): before the application tree's
    /// dialogs pick a window, since X11 reports an iconify with no event.
    pub(crate) fn refresh_minimised_windows(&mut self) {
        let now = std::time::Instant::now();
        for managed in self.windows.values_mut() {
            managed.refresh_minimized(now);
        }
    }

    /// Keep the application tree's `request` until the user comes back: it
    /// was asked with no input while the window it was to open over was
    /// minimised ([`AppDialogPresentation::Owe`]). It is presented on the
    /// first turn a window of the application takes the focus
    /// ([`note_app_focus_gained`](Self::note_app_focus_gained)), over the
    /// window chosen then, and a quit answers it as dismissed
    /// ([`answer_owed_app_dialogs`](Self::answer_owed_app_dialogs)).
    pub(crate) fn owe_app_dialog(&mut self, request: teksilo_core::modal::ModalRequest) {
        self.owed_app_dialogs.push(request);
    }

    /// Take the owed dialogs, oldest first, to present them or owe them
    /// again.
    pub(crate) fn take_owed_app_dialogs(&mut self) -> Vec<teksilo_core::modal::ModalRequest> {
        std::mem::take(&mut self.owed_app_dialogs)
    }

    /// A window of the application took the focus: the dialogs owed are
    /// presented in this turn.
    pub(crate) fn note_app_focus_gained(&mut self) {
        if !self.owed_app_dialogs.is_empty() {
            self.owed_app_dialogs_due = true;
        }
    }

    /// Whether a window took the focus since the owed dialogs were last
    /// looked at; clears it.
    pub(crate) fn take_owed_app_dialogs_due(&mut self) -> bool {
        std::mem::take(&mut self.owed_app_dialogs_due)
    }

    /// The next modal window [`create_window`](Self::create_window) opens is
    /// raised (`true`), or raised only as its owner's focus and the turn's
    /// input decide (`false`).
    pub(crate) fn set_raise_next_modal(&mut self, raise: bool) {
        self.raise_next_modal = raise;
    }

    /// Whether the quit under way waits on an answer, from the
    /// application's guard or a window's: a dialog asked then is the quit's
    /// question, brought to the user however the quit came (a logout, a
    /// Dock menu, a signal), as applications do.
    pub(crate) fn quit_awaits_answer(&self) -> bool {
        self.quit.is_some_and(|quit| quit.awaiting.is_some())
    }

    /// Answer every owed dialog (see [`owe_app_dialog`](Self::owe_app_dialog))
    /// as dismissed, in the application tree that asked for it
    /// ([`answer_unshown_dialog`]), so a `MessageBox` reports its Escape
    /// button (Cancel). Run as a quit completes.
    fn answer_owed_app_dialogs(&mut self, target: &winit::event_loop::ActiveEventLoop) {
        let owed = std::mem::take(&mut self.owed_app_dialogs);
        if owed.is_empty() {
            return;
        }
        let Some(mut tree) = self.take_app_tree() else {
            return;
        };
        {
            let mut ops = WindowOpsImpl::windowless(self, target);
            for request in owed {
                answer_unshown_dialog(request, &mut tree, &mut ops);
            }
        }
        self.reinsert_app_tree(tree);
    }

    /// Keep `window`'s own `request` until the user comes back to it: a
    /// native dialog window its tree asked for with no input while it was
    /// minimised ([`AppDialogPresentation::Owe`]). The first one owed asks
    /// for the user's attention, which lasts until they come back. It is
    /// presented, raised, on the first turn `window` or a dialog of it takes
    /// the focus ([`note_owed_dialogs_focus`](Self::note_owed_dialogs_focus));
    /// `window` closing, by itself or for a quit, answers it as dismissed
    /// ([`close_window_answering`](Self::close_window_answering)). Nothing
    /// when `window` is not open.
    pub(crate) fn owe_window_dialog(
        &mut self,
        window: TeksiloWindowId,
        request: teksilo_core::modal::ModalRequest,
    ) {
        let Some(managed) = self.get_by_teksilo_mut(window) else {
            return;
        };
        if managed.owed_dialogs.is_empty() {
            managed.ask_attention();
        }
        managed.owed_dialogs.push(request);
    }

    /// Whether `window` owes the user dialogs not yet due: a native dialog it
    /// asks for now waits behind them, so the order they were asked in
    /// holds.
    pub(crate) fn owes_window_dialogs(&self, window: TeksiloWindowId) -> bool {
        self.teksilo_to_winit
            .get(&window)
            .and_then(|winit| self.windows.get(winit))
            .is_some_and(|managed| !managed.owed_dialogs.is_empty() && !managed.owed_dialogs_due)
    }

    /// `teksilo_id` took the focus: the user came to it, and to every
    /// window it is a dialog of, so the attention any of them asked for is
    /// withdrawn ([`ManagedWindow::clear_attention`]).
    pub(crate) fn note_attention_answered(&mut self, teksilo_id: TeksiloWindowId) {
        let mut chain = self.owner_chain(teksilo_id);
        chain.push(teksilo_id);
        for window in chain {
            if let Some(managed) = self.get_by_teksilo_mut(window) {
                managed.clear_attention();
            }
        }
    }

    /// `teksilo_id` took the focus: the dialogs it owes, and those every
    /// window it is a dialog of owes, are presented in this turn.
    pub(crate) fn note_owed_dialogs_focus(&mut self, teksilo_id: TeksiloWindowId) {
        let mut chain = self.owner_chain(teksilo_id);
        chain.push(teksilo_id);
        for window in chain {
            if let Some(managed) = self.get_by_teksilo_mut(window)
                && !managed.owed_dialogs.is_empty()
            {
                managed.owed_dialogs_due = true;
            }
        }
    }

    /// Take the owed dialogs of every window that took the focus since
    /// (see [`note_owed_dialogs_focus`](Self::note_owed_dialogs_focus)),
    /// oldest first, to present them.
    pub(crate) fn take_due_owed_dialogs(
        &mut self,
    ) -> Vec<(TeksiloWindowId, Vec<teksilo_core::modal::ModalRequest>)> {
        let mut due = Vec::new();
        for managed in self.windows.values_mut() {
            if std::mem::take(&mut managed.owed_dialogs_due) {
                due.push((
                    managed.teksilo_id,
                    std::mem::take(&mut managed.owed_dialogs),
                ));
            }
        }
        due
    }

    /// The quit stops at `teksilo_id`, whose guard deferred its answer: wait
    /// on it, and make sure the user can see its question. A minimised
    /// window is restored and the dialog raised (or the window, while its
    /// dialog is still queued); the redraw brings the `post_event` that
    /// presents a queued dialog.
    fn await_quit_at(&mut self, teksilo_id: TeksiloWindowId) {
        if let Some(quit) = &mut self.quit {
            quit.awaiting = Some(crate::quit::Awaited::Window(teksilo_id));
        }
        let front = self.frontmost_modal_of(teksilo_id);
        self.bring_forward(front, None);
    }

    /// What the quit sees of the open windows; see
    /// [`QuitWindow`](crate::quit::QuitWindow). A window closing this turn
    /// is not one: it is not asked.
    fn quit_windows(&self) -> Vec<crate::quit::QuitWindow> {
        self.windows
            .values()
            .filter(|managed| !self.is_closing(managed.teksilo_id))
            .map(|managed| {
                let id = managed.teksilo_id;
                let mut modal_depth = 0;
                let mut parent = self.modal_parent_of(id);
                while let Some(up) = parent
                    && modal_depth < 64
                {
                    modal_depth += 1;
                    parent = self.modal_parent_of(up);
                }
                crate::quit::QuitWindow {
                    id,
                    modal_depth,
                    recency: self.recency.stamp(id).unwrap_or(0),
                    deferred: self.deferred_closes.contains_key(&id),
                }
            })
            .collect()
    }

    /// Carry the quit under way as far as it goes now: close the windows
    /// that agree, abandon the quit at one whose guard vetoes, and stop at
    /// one whose answer is deferred. Runs after every
    /// [`process_pending`](Self::process_pending), outside any dispatch, so
    /// every window is in the map.
    pub fn advance_quit(&mut self, target: &winit::event_loop::ActiveEventLoop) {
        use crate::quit::QuitStep;

        // Each pass closes a window or stops; the bound only guards against
        // guards that open a window for every one they close.
        for _ in 0..4096 {
            let Some(quit) = self.quit else {
                return;
            };
            // A guard may have closed another window (by id): that window
            // is closing, not asked, and closes before the next step, so
            // the quit counts it gone.
            if !self.pending_closes.is_empty() {
                self.process_closes(target);
                continue;
            }
            match crate::quit::step(&quit, &self.quit_windows()) {
                QuitStep::Complete => {
                    // A dialog the application tree still owes the user is
                    // answered as its window would have been closed; what
                    // its answer asks for is carried out before the quit
                    // completes.
                    if !self.owed_app_dialogs.is_empty() {
                        self.answer_owed_app_dialogs(target);
                        continue;
                    }
                    self.quit = None;
                    self.quit_completed = true;
                    self.reply_to_quit(crate::quit::QuitOutcome::Completed);
                    return;
                }
                QuitStep::Wait => return,
                QuitStep::AskApplication => {
                    let response = self.evaluate_app_quit_guard(target);
                    let same_quit = self
                        .quit
                        .is_some_and(|now| now.generation == quit.generation);
                    if !same_quit {
                        continue;
                    }
                    // An overruling quit asked so the application could
                    // save; its answer does not stop it.
                    let response = match response {
                        _ if quit.mode == crate::quit::QuitMode::Overruling => CloseResponse::Close,
                        response => response,
                    };
                    match response {
                        CloseResponse::Close => {
                            if let Some(quit) = &mut self.quit {
                                quit.application_agreed = true;
                            }
                        }
                        CloseResponse::Veto => {
                            self.end_quit();
                            return;
                        }
                        CloseResponse::Defer(decision) => {
                            self.app_quit_deferral = Some(self.wake_on_answer(&decision));
                            if let Some(quit) = &mut self.quit {
                                quit.awaiting = Some(crate::quit::Awaited::Application);
                            }
                            return;
                        }
                    }
                }
                QuitStep::Adopt(id) => {
                    self.await_quit_at(id);
                    return;
                }
                QuitStep::Ask(id) => {
                    let response = if quit.mode == crate::quit::QuitMode::Forced {
                        CloseResponse::Close
                    } else {
                        self.evaluate_close_guard(id, target)
                    };
                    // The guard may have changed the quit: cancelled it,
                    // or made it forced. Only the quit that asked acts on
                    // its answer; the window's own close follows it anyway.
                    let same_quit = self
                        .quit
                        .is_some_and(|now| now.generation == quit.generation);
                    // An overruling quit asked so the window could save; a
                    // refusal or a deferred answer is overruled, and the
                    // dialog a deferring guard presented answers Cancel as
                    // its window closes.
                    let response = match response {
                        _ if same_quit && quit.mode == crate::quit::QuitMode::Overruling => {
                            CloseResponse::Close
                        }
                        response => response,
                    };
                    match response {
                        CloseResponse::Close => self.close_window_answering(id, target),
                        CloseResponse::Veto => {
                            if same_quit {
                                self.end_quit();
                                return;
                            }
                        }
                        CloseResponse::Defer(decision) => {
                            self.defer_close(id, &decision);
                            if same_quit {
                                self.await_quit_at(id);
                                return;
                            }
                        }
                    }
                }
            }
        }
        // In every build, once per quit: the symptom (an application that
        // will not quit) is the user's, in a release build too, and this is
        // the only account of it.
        if !self.quit_bound_reported {
            self.quit_bound_reported = true;
            eprintln!(
                "teksilo-app: a quit was still going after 4096 steps, its close guards opening \
                 a window for each one closed; it goes on at the next turn"
            );
        }
    }

    /// Run the close guard for `teksilo_id` (if it declared one) and
    /// return its answer.
    ///
    /// A window with no guard answers `Close` without building an
    /// `EventContext`. Otherwise the guard runs with a real
    /// [`EventContext`](teksilo_core::widget::EventContext) for the
    /// window's own tree (so it can open a confirmation dialog, set
    /// signals, fire intents…), and the verdict is:
    ///
    /// 1. [`can_close`](teksilo_core::WindowConfig::can_close) sugar: a
    ///    `false` signal vetoes, or, with
    ///    [`on_close_blocked`](teksilo_core::WindowConfig::on_close_blocked)
    ///    set, defers on the decision that callback answers.
    /// 2. otherwise the
    ///    [`on_close_requested`](teksilo_core::WindowConfig::on_close_requested)
    ///    guard's [`CloseResponse`].
    /// 3. otherwise `Close`.
    ///
    /// Strictly per-window: only `teksilo_id`'s own guard is consulted.
    fn evaluate_close_guard(
        &mut self,
        teksilo_id: TeksiloWindowId,
        target: &winit::event_loop::ActiveEventLoop,
    ) -> CloseResponse {
        let Some(&winit_id) = self.teksilo_to_winit.get(&teksilo_id) else {
            // Unknown / already-gone window: let `close_window` no-op.
            return CloseResponse::Close;
        };
        // Fast path: no guard configured → close immediately, without
        // paying for an EventContext.
        match self.windows.get(&winit_id) {
            Some(managed) if managed.close_guard.is_none() && managed.can_close.is_none() => {
                return CloseResponse::Close;
            }
            None => return CloseResponse::Close,
            _ => {}
        }
        // Take the window out of the map so we can borrow `&mut self`
        // for `WindowOpsImpl` while still holding the window's tree.
        let Some(mut managed) = self.take_managed(winit_id) else {
            return CloseResponse::Close;
        };

        let current_arc = Some(managed.platform_window.window_arc());

        // Clone the guard handles so the dispatch closure captures them
        // disjointly from `managed.tree`, which `run_with_event_context`
        // borrows mutably.
        let close_guard = managed.close_guard.clone();
        let can_close = managed.can_close.clone();
        let on_close_blocked = managed.on_close_blocked.clone();

        let mut response = CloseResponse::Close;
        {
            let mut ops = WindowOpsImpl::new(self, target, teksilo_id, current_arc);
            managed.tree.run_with_event_context(&mut ops, |ctx| {
                response = close_verdict(&can_close, &on_close_blocked, &close_guard, ctx);
            });
        }

        self.reinsert_managed(winit_id, managed);
        response
    }

    /// Get a mutable ManagedWindow for a winit WindowId.
    pub(crate) fn get_by_winit_mut(
        &mut self,
        id: winit::window::WindowId,
    ) -> Option<&mut ManagedWindow> {
        self.windows.get_mut(&id)
    }

    /// Temporarily remove a managed window from the map. Used by
    /// `TeksiloAppHandler::dispatch_in_window` so the handler's
    /// `&mut tree` borrow does not collide with
    /// [`WindowOpsImpl`]'s `&mut WindowManager` borrow.
    /// The caller must pair this with
    /// [`reinsert_managed`](Self::reinsert_managed) before the
    /// enclosing winit event returns.
    pub(crate) fn take_managed(&mut self, id: winit::window::WindowId) -> Option<ManagedWindow> {
        let managed = self.windows.remove(&id)?;
        self.taken_out.insert(
            managed.teksilo_id,
            TakenOut {
                focused: managed.focused,
                minimized: managed.minimized,
            },
        );
        Some(managed)
    }

    /// The window `teksilo_id`, out of the map while its handler runs, was
    /// restored from minimised: the manager knows it at once, and the
    /// window's own record when it is [put back](Self::reinsert_managed).
    pub(crate) fn note_taken_out_restored(&mut self, teksilo_id: TeksiloWindowId) {
        if let Some(taken) = self.taken_out.get_mut(&teksilo_id) {
            taken.minimized = false;
        }
    }

    /// Re-insert a `ManagedWindow` previously extracted via
    /// [`take_managed`](Self::take_managed).
    pub(crate) fn reinsert_managed(
        &mut self,
        id: winit::window::WindowId,
        mut managed: ManagedWindow,
    ) {
        // Restored while out (`WindowOps::focus_window` on the dispatching
        // window): what the manager knows follows.
        if let Some(taken) = self.taken_out.remove(&managed.teksilo_id)
            && managed.minimized
            && !taken.minimized
        {
            managed.minimized = false;
            managed.minimized_probed_at = None;
            managed.sync_hidden();
        }
        self.windows.insert(id, managed);
        #[cfg(all(unix, not(target_os = "macos")))]
        if self.wm_state_watch_stale {
            self.sync_wm_state_watch();
        }
    }

    /// Winit ids of every window whose tree has post-mount actions queued
    /// (via `BuildContext::run_after_mount`) waiting to run. Drained by
    /// `TeksiloAppHandler::process_pending_mount_actions`. Modal-blocked
    /// windows are excluded so their actions (e.g. a WebView opening a native
    /// engine subview) stay queued until the modal closes — a native surface
    /// must not appear over a modal-blocked parent.
    pub(crate) fn winit_ids_with_pending_mount_actions(&self) -> Vec<winit::window::WindowId> {
        self.windows
            .iter()
            .filter(|(_, m)| m.tree.has_pending_mount_actions() && !self.is_blocked(m.teksilo_id))
            .map(|(id, _)| *id)
            .collect()
    }

    /// `pub(crate)` access to the windows map used by
    /// [`WindowOpsImpl`].
    pub(crate) fn windows_map(&self) -> &HashMap<winit::window::WindowId, ManagedWindow> {
        &self.windows
    }

    /// `pub(crate)` access to the teksilo→winit id map used by
    /// [`WindowOpsImpl`].
    pub(crate) fn teksilo_to_winit_map(
        &self,
    ) -> &HashMap<TeksiloWindowId, winit::window::WindowId> {
        &self.teksilo_to_winit
    }

    /// The managed window behind a [`TeksiloWindowId`], mutably. `None` once
    /// the window has closed — both lookups are fallible, so a stale id is a
    /// miss rather than a panic.
    pub(crate) fn get_by_teksilo_mut(&mut self, id: TeksiloWindowId) -> Option<&mut ManagedWindow> {
        let winit_id = self.teksilo_to_winit.get(&id).copied()?;
        self.windows.get_mut(&winit_id)
    }

    /// Get the TeksiloWindowId for a winit WindowId.
    pub fn teksilo_id_for_winit(&self, id: winit::window::WindowId) -> Option<TeksiloWindowId> {
        self.windows.get(&id).map(|w| w.teksilo_id)
    }

    /// Find a window by its string ID.
    pub fn find_window(&self, string_id: &str) -> Option<TeksiloWindowId> {
        self.string_to_id.get(string_id).copied()
    }

    /// Whether a window is blocked by a modal child.
    pub fn is_blocked(&self, teksilo_id: TeksiloWindowId) -> bool {
        self.modal_blocked.contains_key(&teksilo_id)
    }

    /// The modal window currently blocking `teksilo_id`, if one is and it
    /// is not about to close.
    pub fn blocking_modal_child(&self, teksilo_id: TeksiloWindowId) -> Option<TeksiloWindowId> {
        self.modal_blocked
            .get(&teksilo_id)
            .copied()
            .filter(|child| !self.is_closing(*child))
    }

    /// Pull focus back to the modal child of a blocked parent the user just
    /// clicked, the "this window is waiting on that one" nudge. A child
    /// minimised (where the window manager offered to minimise a dialog)
    /// comes back first.
    pub fn refocus_modal_child(&mut self, blocked_parent: TeksiloWindowId) {
        self.raise_modal_child(blocked_parent, true);
    }

    /// Bring forward the modal child blocking `blocked_parent`, restored if
    /// minimised. With `ask_os`, the window system is asked whether it is
    /// minimised (an X11 round trip, worth it for a click); otherwise only
    /// what the manager knows counts.
    fn raise_modal_child(&mut self, blocked_parent: TeksiloWindowId, ask_os: bool) {
        let Some(child_id) = self.blocking_modal_child(blocked_parent) else {
            return;
        };
        let Some(child) = self.get_by_teksilo_mut(child_id) else {
            return;
        };

        // Re-surface the modal relative to its owner via focus alone. The
        // cross-platform `window_activation::raise` helper raises on
        // X11/Windows/macOS and degrades to an attention request on Wayland
        // (where raising an existing window without a token is a no-op). Do NOT
        // call `set_window_level(AlwaysOnTop)`: it floats the modal above *all*
        // windows (every app) for its lifetime, and on a Win32 owned window it
        // also stalls the message pump (paint events stop until a focus/resize
        // forces a redraw) — exactly the failure the creation path documents
        // and avoids. The owner / transient-parent relationship already keeps
        // the modal above its parent.
        if ask_os {
            child.restore();
        } else {
            child.restore_known();
        }
        let window = child.platform_window.window();
        raise_window(window, None);
        // Hidden with the window it blocks (a macOS child window while its
        // parent comes back from the Dock), it was not raised: owe it.
        if window.is_visible() == Some(false) {
            self.raise_owed.insert(child_id);
        }
    }

    /// Whether `id` owes a raise the window system did not take (see
    /// [`note_dialog_sign`](Self::note_dialog_sign)). Read by the winit
    /// loop tests, which only a Linux test build has.
    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn owes_raise(&self, id: TeksiloWindowId) -> bool {
        self.raise_owed.contains(&id)
    }

    /// `id` showed a sign of being on screen: pay the raise it owes, if
    /// any, once it can take it. See [`owed_raise`]. Owed only while the
    /// application is active: with no window of it focused, the raise is
    /// dropped (see [`drop_owed_raises_if_inactive`](Self::drop_owed_raises_if_inactive)).
    pub(crate) fn note_dialog_sign(&mut self, id: TeksiloWindowId, sign: DialogSign) {
        self.pay_owed_attach(id);
        self.drop_owed_raises_if_inactive();
        if !self.raise_owed.contains(&id) {
            return;
        }
        let Some(dialog) = self.get_by_teksilo_mut(id) else {
            self.raise_owed.remove(&id);
            return;
        };
        let window = dialog.platform_window.window();
        match owed_raise(sign, window.is_visible()) {
            OwedRaise::Keep => {}
            OwedRaise::Raise => {
                raise_window(window, None);
                self.raise_owed.remove(&id);
            }
            OwedRaise::Forget => {
                self.raise_owed.remove(&id);
            }
        }
    }

    /// macOS: attach `id` to the window it blocks as a child window, if
    /// the attach is owed ([`attach_owed`](Self::attach_owed)) and that
    /// window is no longer miniaturised. Run on each of `id`'s signs of
    /// being on screen. Nothing is ever owed elsewhere.
    fn pay_owed_attach(&mut self, id: TeksiloWindowId) {
        let Some(&owner) = self.attach_owed.get(&id) else {
            return;
        };
        let (Some(owner_window), Some(dialog_window)) = (
            self.parent_window(Some(owner), None),
            self.parent_window(Some(id), None),
        ) else {
            if self.winit_id_for_teksilo(owner).is_none() {
                self.attach_owed.remove(&id);
            }
            return;
        };
        if owner_window.is_minimized() == Some(true) {
            return;
        }
        teksilo_platform::attach_child_window(owner_window, dialog_window);
        self.attach_owed.remove(&id);
    }

    /// Forget every raise a dialog is owed when no window of the application
    /// has the focus: the user went to another application while the window
    /// system still hid the dialog, and a raise paid now would take the focus
    /// back from it. Run every event-loop turn, and before a raise is paid.
    pub(crate) fn drop_owed_raises_if_inactive(&mut self) {
        if !self.raise_owed.is_empty() && !self.windows.values().any(|w| w.focused) {
            self.raise_owed.clear();
        }
    }

    /// `id` took the focus. A window blocked by a modal child brings the
    /// child forward, as a click in it does: a click on its title bar,
    /// Alt+Tab and its taskbar button reach it only as focus. Asks the window
    /// system nothing: the child is restored only if the manager knows it
    /// minimised.
    ///
    /// At the end of the turn ([`refocus_after_turn`](Self::refocus_after_turn)),
    /// not now: on macOS a raise made while the focus event is handled (a
    /// click on the Dock icon activating the application, a click on the
    /// blocked window's title bar) left the blocked window key, AppKit
    /// finishing the activation after it, and the dialog in front without
    /// the keyboard (traced on the Mac; the same raise at the end of the
    /// turn makes the dialog key).
    pub(crate) fn note_focus_gained(&mut self, id: TeksiloWindowId) {
        if !self.refocus_due.contains(&id) {
            self.refocus_due.push(id);
        }
    }

    /// The raises [`note_focus_gained`](Self::note_focus_gained) left for
    /// the end of the turn, each made if its window still has the focus.
    /// Run by `about_to_wait`.
    pub(crate) fn refocus_after_turn(&mut self) {
        for id in std::mem::take(&mut self.refocus_due) {
            if self
                .get_by_teksilo_mut(id)
                .is_some_and(|managed| managed.focused)
            {
                self.raise_modal_child(id, false);
            }
        }
    }

    /// Watch the window state of exactly the windows in a modal pair (each
    /// dialog and the window it blocks), on X11, where the window system
    /// does not keep a dialog with its owner and winit does not report a
    /// minimise ([`WmStateWatch`](teksilo_platform::x11::WmStateWatch)). The
    /// watch starts with the first dialog, once the event proxy is installed,
    /// and is kept; with no dialog open it watches nothing and its thread
    /// sleeps. Run whenever a dialog opens or a window closes. A no-op
    /// elsewhere.
    pub(crate) fn sync_wm_state_watch(&mut self) {
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let ids: std::collections::HashSet<TeksiloWindowId> = self
                .modal_parents
                .iter()
                .flat_map(|(dialog, owner)| [*dialog, *owner])
                .collect();
            if self.wm_state_watch.is_none() {
                // Only for X11 windows: a Wayland session has nothing to
                // watch (and may have an X server, Xwayland, to connect to).
                let on_x11 = self.windows.values().any(|managed| {
                    ids.contains(&managed.teksilo_id)
                        && winit::raw_window_handle::HasDisplayHandle::display_handle(
                            managed.platform_window.window(),
                        )
                        .is_ok_and(|display| {
                            teksilo_platform::window_system_for_display_handle(&display.as_raw())
                                == teksilo_platform::WindowSystem::X11
                        })
                });
                if !on_x11 {
                    return;
                }
                let Some(proxy) = self.event_proxy.clone() else {
                    return;
                };
                self.wm_state_watch = teksilo_platform::x11::WmStateWatch::new(move || {
                    teksilo_core::AppEventPoster::post_external(
                        &proxy,
                        Box::new(crate::app::WmStateChanged),
                    );
                });
            }
            let Some(watch) = &self.wm_state_watch else {
                return;
            };
            let present: Vec<&winit::window::Window> = self
                .windows
                .values()
                .filter(|managed| ids.contains(&managed.teksilo_id))
                .map(|managed| managed.platform_window.window())
                .collect();
            // The window a handler runs in is out of the map meanwhile (a
            // dialog opened from its owner's handler): synced again when it
            // is put back.
            self.wm_state_watch_stale = present.len() < ids.len();
            watch.watch_only(present);
        }
    }

    /// A watched window's state changed
    /// ([`sync_wm_state_watch`](Self::sync_wm_state_watch)): read again
    /// whether each dialog and each window a dialog blocks is minimised, from
    /// its own `_NET_WM_STATE` (one round trip each), so that
    /// [`follow_owners`](Self::follow_owners), which runs in the
    /// `about_to_wait` this event is followed by, acts on it. A dialog asked
    /// to follow its owner is marked minimised here, once its own state says
    /// the window manager minimised it.
    pub(crate) fn reread_wm_states(&mut self, now: std::time::Instant) {
        #[cfg(test)]
        {
            self.wm_state_rereads += 1;
        }
        let ids: std::collections::HashSet<TeksiloWindowId> = self
            .modal_parents
            .iter()
            .flat_map(|(dialog, owner)| [*dialog, *owner])
            .collect();
        for managed in self.windows.values_mut() {
            if !ids.contains(&managed.teksilo_id) {
                continue;
            }
            let minimised = managed.platform_window.window().is_minimized() == Some(true);
            managed.minimized_probed_at = Some(now);
            if managed.minimized != minimised {
                managed.minimized = minimised;
                managed.sync_hidden();
            }
        }
    }

    /// Minimise and restore each modal dialog with its owner, where the
    /// window system does not ([`OS_KEEPS_DIALOGS_WITH_OWNER`]); see
    /// [`follow_owner`]. Run every event-loop turn, from what the manager
    /// knows of each window: on X11 an owner's minimise reaches it through
    /// the [window-state watch](Self::sync_wm_state_watch), which wakes the
    /// loop, so nothing is polled. A dialog is asked of the window manager to
    /// minimise and counts as minimised only once its own state says so
    /// ([`reread_wm_states`](Self::reread_wm_states)): a window manager may
    /// refuse (i3 marks the windows of a hidden tab `_NET_WM_STATE_HIDDEN`
    /// and minimises nothing), and a window marked minimised draws nothing.
    /// Owners are followed before their dialogs, so a dialog of a dialog
    /// restores in the same turn as the dialog it follows, and minimises in
    /// the turn that dialog's own minimise is confirmed. On Wayland winit
    /// cannot tell a window is minimised, so nothing follows there.
    pub(crate) fn follow_owners(&mut self) {
        if OS_KEEPS_DIALOGS_WITH_OWNER || self.modal_parents.is_empty() {
            return;
        }
        let mut pairs: Vec<(usize, TeksiloWindowId, TeksiloWindowId)> = self
            .modal_parents
            .iter()
            .map(|(dialog, owner)| (self.owner_chain(*dialog).len(), *dialog, *owner))
            .collect();
        pairs.sort_by_key(|(depth, _, _)| *depth);
        for (_, dialog, owner) in pairs {
            let Some(owner_minimised) = self.get_by_teksilo_mut(owner).map(|w| w.minimized) else {
                continue;
            };
            let seen = self.owners_seen.get(&dialog).copied().unwrap_or_default();
            let mut went_with = seen.went_with;
            match follow_owner(seen.minimised, owner_minimised, went_with, false) {
                FollowOwner::Nothing => {}
                FollowOwner::Minimise => {
                    if let Some(dialog_window) = self.get_by_teksilo_mut(dialog) {
                        dialog_window.platform_window.window().set_minimized(true);
                        went_with = true;
                    }
                }
                FollowOwner::Restore => {
                    if let Some(dialog_window) = self.get_by_teksilo_mut(dialog) {
                        dialog_window.restore_and_raise();
                    }
                    went_with = false;
                }
            }
            self.owners_seen.insert(
                dialog,
                OwnerSeen {
                    minimised: owner_minimised,
                    went_with,
                },
            );
        }
    }

    /// Broadcast a theme change to all windows.
    ///
    /// `ThemeMode` is `App`-level state, so a user-driven theme set under
    /// `FollowSystem`/`Native` is last-writer-wins against the next OS theme
    /// event (`handle_theme_changed`). The default `Manual` mode ignores OS
    /// events, so an app that wants user theme choices to stick should stay on
    /// `Manual` (the default).
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme.clone();
        self.theme_generation += 1;
        self.os_theme = None;
        for managed in self.windows.values_mut() {
            managed.os_theme = None;
            managed.tree.set_theme(theme.clone());
            // The translator holds the input tokens, not the theme, so a
            // theme swap has to re-install them or the touch kill switch and
            // the wheel-notch scale stay at whatever the old theme said.
            managed.translation_state.set_input_tokens(theme.input);
        }
        self.sync_app_tree_theme();
    }

    /// Broadcast a user text-scale change to all windows. Stores the factor so
    /// windows created later inherit it, then re-scales every existing tree's
    /// text without rebuilding.
    pub fn set_text_scale(&mut self, factor: f32) {
        self.user_text_scale = factor;
        for managed in self.windows.values_mut() {
            managed.tree.set_user_text_scale(factor);
        }
        if let Some(tree) = &mut self.app_tree {
            tree.set_user_text_scale(factor);
        }
    }

    /// Seed the user text-scale factor before the first window opens. Called by
    /// `TeksiloAppHandler` after reading `teksilo_settings::TEXT_SCALE_KEY`, so
    /// every initially-created tree starts at the persisted scale.
    pub fn set_initial_text_scale(&mut self, factor: f32) {
        self.user_text_scale = factor;
    }

    /// Re-query the OS accessibility preferences ("increase contrast", "reduce
    /// motion", text scale, screen reader) and, if they changed since startup /
    /// the last refresh, apply them to every open window's tree.
    ///
    /// Cost note: on Linux the contrast, reduced-motion and screen-reader
    /// flags are read over the session bus with zbus (the desktop portal, and
    /// AT-SPI's status object on `org.a11y.Bus`), on one connection the
    /// process keeps, and the text scale and the GNOME fallbacks run the
    /// `gsettings` tool as a subprocess. A bus that answers costs a few
    /// milliseconds per read. A bus, or a service on it, that does not answer
    /// costs up to the reader's 500 ms call timeout for the first read that
    /// meets it, after which the reader leaves that bus or service alone for
    /// a 5 s quiet period: on such a session a focus change can be held up by
    /// about half a second for each silent service, once per quiet period,
    /// and by nothing in between. It is on the window-focus path, not an idle
    /// poll, so it costs nothing at rest. Lets a runtime toggle
    /// of these settings take effect without restarting the app (WCAG / EN
    /// 301 549 §11.7). Driven event-first — from `WindowEvent::Focused` when a
    /// window gains focus — so there is no idle polling wakeup. Returns `true`
    /// if anything changed (so the caller can request a redraw).
    pub fn refresh_accessibility_preferences(&mut self) -> bool {
        self.refresh_accessibility_preferences_from(AccessibilityPreferences::query)
    }

    /// [`refresh_accessibility_preferences`](Self::refresh_accessibility_preferences),
    /// reading the preferences through `query`.
    fn refresh_accessibility_preferences_from(
        &mut self,
        query: fn() -> AccessibilityPreferences,
    ) -> bool {
        let fresh = query();
        if fresh == self.a11y_prefs {
            return false;
        }
        let hc = fresh.high_contrast;
        let rm = fresh.reduced_motion;
        let ts = fresh.text_scale_factor;
        let sr = fresh.screen_reader;
        self.a11y_prefs = fresh;
        for managed in self.windows.values_mut() {
            managed.tree.set_accessibility_preferences(hc, rm, ts);
            // A screen reader starting or stopping while the app runs is the
            // case this refresh exists for: `AccessibilityPreferences` compares
            // on the field, so the early return above no longer swallows it.
            managed.tree.set_screen_reader_state(sr);
        }
        if let Some(tree) = &mut self.app_tree {
            tree.set_accessibility_preferences(hc, rm, ts);
            tree.set_screen_reader_state(sr);
        }
        true
    }

    /// Broadcast a locale switch to all windows. Updates the i18n manager
    /// (incrementing the version signal) and seeds each tree with the new
    /// locale and layout direction. No-op if no `I18nConfig` was registered.
    pub fn set_locale(&mut self, locale: teksilo_i18n::LanguageIdentifier) {
        let Some((outcome, new_dir)) = teksilo_i18n::thread_local::with_active(|mgr| {
            let outcome = mgr.set_locale(locale.clone());
            (outcome, mgr.direction_signal().get())
        }) else {
            return;
        };
        for tree in self.trees_mut() {
            if outcome.direction_changed {
                tree.set_layout_direction(new_dir);
            }
            tree.set_locale(locale.to_string());
        }
    }

    /// Get the current shared theme.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Get the OS-level accessibility preferences (queried at startup).
    pub fn accessibility_preferences(&self) -> &AccessibilityPreferences {
        &self.a11y_prefs
    }

    /// Get the TeksiloWindowId of the first (primary) window.
    /// Falls back to a synthetic ID when no windows are open yet.
    pub fn primary_window_id(&self) -> TeksiloWindowId {
        self.teksilo_to_winit
            .keys()
            .copied()
            .min_by_key(|id| id.raw())
            .unwrap_or(TeksiloWindowId::new(0))
    }

    /// The window opened or focused last, `None` with no window open.
    ///
    /// Deterministic where the windows' `focused` flags are not: every
    /// window is born claiming focus, so two can claim it at once, and the
    /// window map has no order. A window counts from the moment it opens
    /// and again each time the OS reports it focused; when it closes, the
    /// window used before it takes over. The window a handler is running in
    /// is taken out of the map meanwhile, and still counts here.
    pub fn most_recent_window(&self) -> Option<TeksiloWindowId> {
        self.recency.most_recent()
    }

    /// The window that has the OS focus now (macOS's key window), `None`
    /// when no open window does: of the windows whose last focus report
    /// said focused, the one focused last. A window is born claiming focus
    /// until the OS says otherwise, and the most recent claim is the one the
    /// OS made last. A window a handler is running in is out of the map
    /// meanwhile and does not count.
    pub(crate) fn focused_window(&self) -> Option<TeksiloWindowId> {
        self.recency.most_recent_where(|id| {
            self.teksilo_to_winit
                .get(&id)
                .and_then(|winit| self.windows.get(winit))
                .is_some_and(|window| window.focused)
        })
    }

    /// Record that the OS reported `teksilo_id` focused, making it the
    /// [most recent window](Self::most_recent_window).
    pub(crate) fn note_window_focused(&mut self, teksilo_id: TeksiloWindowId) {
        self.recency.touch(teksilo_id);
    }

    /// Every window other than `except` that still claims focus. Only one
    /// window holds the OS focus at a time, so once `except` reports it,
    /// each of these carries the flag every window is born with and that no
    /// event corrected (a window the compositor opened without focus, on
    /// Wayland with no activation token).
    pub(crate) fn other_focus_claims(
        &self,
        except: winit::window::WindowId,
    ) -> Vec<winit::window::WindowId> {
        self.windows
            .iter()
            .filter(|(id, managed)| **id != except && managed.focused)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Number of active windows.
    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    /// Whether no windows remain. Whether the application then quits is its
    /// [`QuitPolicy`](crate::QuitPolicy)'s call.
    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    /// Iterate over all managed windows.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &ManagedWindow> {
        self.windows.values()
    }

    /// Mutably iterate over all managed windows. During a redraw the
    /// current window is taken out of the map (`take_managed`), so this
    /// yields every OTHER window — which is exactly what the glyph-atlas
    /// eviction recovery wants when broadcasting paint invalidation.
    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut ManagedWindow> {
        self.windows.values_mut()
    }

    /// Iterate the `TeksiloWindowId` of every managed window. Used only by
    /// the loop test: the automation `list_windows` tool (debug-only
    /// `automation` feature) is served from [`iter`](Self::iter) instead,
    /// because it also needs each window's `string_id` label and current
    /// title.
    #[allow(dead_code)]
    pub(crate) fn teksilo_ids(&self) -> impl Iterator<Item = TeksiloWindowId> + '_ {
        self.windows.values().map(|m| m.teksilo_id)
    }

    /// The window `parent` names: `dispatching` when it is that one (out of
    /// the map while its handler runs), otherwise the open window with that id.
    fn parent_window<'w>(
        &'w self,
        parent: Option<TeksiloWindowId>,
        dispatching: Option<(TeksiloWindowId, &'w winit::window::Window)>,
    ) -> Option<&'w winit::window::Window> {
        let parent = parent?;
        if let Some((id, window)) = dispatching
            && id == parent
        {
            return Some(window);
        }
        let winit_id = self.winit_id_for_teksilo(parent)?;
        Some(self.windows.get(&winit_id)?.platform_window.window())
    }

    /// Get the winit WindowId for a TeksiloWindowId.
    pub fn winit_id_for_teksilo(
        &self,
        teksilo_id: TeksiloWindowId,
    ) -> Option<winit::window::WindowId> {
        self.teksilo_to_winit.get(&teksilo_id).copied()
    }

    /// Get the platform title bar host for a window, if the window opted
    /// into custom chrome via
    /// `WindowConfig::decorations(DecorationsMode::CustomChrome)` and the
    /// platform supports it. Returns `None` for windows that use native
    /// decorations or run on a window system without custom chrome support
    /// (X11 under a window manager that does not implement
    /// `_NET_WM_MOVERESIZE`).
    pub fn title_bar_host(
        &self,
        teksilo_id: TeksiloWindowId,
    ) -> Option<Rc<dyn PlatformTitleBarHost>> {
        let winit_id = self.teksilo_to_winit.get(&teksilo_id).copied()?;
        self.windows
            .get(&winit_id)
            .and_then(|w| w.title_bar_host.clone())
    }

    /// Drop what the application holds, for when the event loop ends: every
    /// window's widget tree and the application tree (with every widget and
    /// the closures they captured), the windows' guards and callbacks, and
    /// the application's state, so each service's `Drop` runs (an analytics
    /// adapter's shutdown flush, a watcher thread's join). Run by winit's
    /// `exiting`, which on macOS is the last code to run when AppKit ends
    /// the process (a Dock Quit, a logout, an Apple-event quit). The
    /// platform windows stay, each with an empty tree, until the handler is
    /// dropped. Idempotent.
    pub(crate) fn shut_down_app_state(&mut self) {
        for managed in self.windows.values_mut() {
            drop(std::mem::replace(&mut managed.tree, WidgetTree::new()));
            managed._persist_handles.clear();
            managed.close_guard = None;
            managed.can_close = None;
            managed.on_close_blocked = None;
            managed.on_removed = None;
            managed.modal_on_dismiss = None;
            managed.owed_dialogs.clear();
        }
        self.app_tree = None;
        self.app_quit_guard = None;
        self.owed_app_dialogs.clear();
        self.orphan_intents.clear();
        self.app_menu_declarations = None;
        self.app_context_template = None;
    }

    /// Make every window's waker a no-op, for when the event loop ends: winit
    /// panics on X11 when a window is asked for a redraw after that, and a
    /// thread may keep waking a window the application still holds.
    /// Idempotent.
    pub(crate) fn disconnect_redraw_wakers(&self) {
        for managed in self.windows.values() {
            managed.platform_window.disconnect_redraw_waker();
        }
    }

    /// Request redraw on all windows.
    pub fn request_redraw_all(&self) {
        for managed in self.windows.values() {
            managed.request_redraw();
        }
    }

    /// Request redraw only on windows whose next frame deadline has been
    /// reached (`<= now`). Used at the animation `ResumeTimeReached` wake so
    /// that a single animating window (e.g. a blinking caret) does NOT force a
    /// redraw of every other window.
    ///
    /// The blanket `request_redraw_all()` here was a cross-window over-redraw:
    /// a wasted-power bug on every platform (an inactive, non-animating window
    /// repainted at 60 Hz), and on Windows a *correctness* bug — winit services
    /// only one window's `RedrawRequested` per event-loop iteration, so an
    /// inactive window flooded with redraws it never wins is starved of its own
    /// pending repaint and freezes on its last active frame (caret stuck,
    /// colours not desaturated). Targeting only due windows removes both.
    ///
    /// Due means the tree's own timer deadline. What the request becomes is
    /// the window's gate's to decide: a redraw for a shown window, a
    /// non-visual tick, rate-limited, for one that draws nothing.
    pub fn request_redraw_due(&self, now: std::time::Instant) {
        for managed in self.windows.values() {
            // `take_retry` first: it clears the retry it reports, so it runs
            // whether or not a timer is due too.
            let retry = managed.redraw.take_retry(now);
            if retry
                || managed
                    .tree
                    .next_timer_deadline()
                    .is_some_and(|deadline| deadline <= now)
            {
                managed.request_redraw();
            }
        }
    }

    /// Reconcile every window's reactive (`Signal`-bound) state and
    /// request redraw ONLY on the ones that come out of that with pending
    /// layout or paint work (`WidgetTree::needs_render`). Returns how many
    /// windows were poked, purely so a caller can log/trace it.
    ///
    /// # The problem
    ///
    /// A `Signal` mutation made by a handler in one window's dispatch
    /// (e.g. writing to an app-level `Signal` a *sibling* window's widget
    /// also reads) is supposed to make that sibling dirty too — that's the
    /// whole point of sharing a `Signal` across windows. But only the
    /// dispatching window's own event-handling path calls
    /// `request_redraw()` on itself (see the `WindowEvent::CursorMoved` /
    /// `MouseInput` / `KeyboardInput` arms); nothing tells winit to
    /// schedule a `RedrawRequested` for the sibling, so it shows a stale
    /// frame until the user focuses it (which finally earns it a paint).
    ///
    /// # Why this can't just check `needs_render()`
    ///
    /// A `Signal` write only advances a change generation on the signal
    /// itself (a deliberately lazy design — a signal has no reference
    /// back into any `WidgetTree`'s arena to mark node-level dirty bits
    /// synchronously). That generation is only compared against what
    /// each window's `BindingRegistry` last acted on, and walked into
    /// `arena.needs_layout` / `needs_paint` — i.e. into what
    /// `needs_render()` actually reads — by `WidgetTree`'s internal
    /// `process_state_changes` step, which today runs *only* at the top
    /// of that tree's own `layout()`. A window's own
    /// `layout()` runs *only* as part of handling its own
    /// `RedrawRequested` (see `handle_redraw_requested` in `app.rs`). So a
    /// sibling window that never redraws never reconciles its bindings
    /// either — its `needs_render()` reads `false` forever, not just
    /// "until it happens to repaint", because nothing ever performed the
    /// walk that would make it `true`. Checking `needs_render()` without
    /// reconciling first would make this method a permanent no-op for
    /// exactly the case it exists to fix.
    ///
    /// That's why a window's `tree.layout()` is called here first, at
    /// its OWN current size (`proposal_changed` stays `false`), whenever
    /// [`WidgetTree::needs_reconcile`] says reconciling could change the
    /// answer. This is the only place in the framework doing this
    /// specific reconciliation for a plain app-level `Signal` (the
    /// theme/locale/text-scale/follow-system broadcasts sidestep the
    /// whole problem by mutating each tree directly through
    /// `&mut self.windows`, not through a shared `Signal`, so they
    /// always know synchronously that every window needs a redraw).
    ///
    /// # Why the reconcile is gated rather than unconditional
    ///
    /// It was unconditional at first, on the grounds that `layout()`
    /// short-circuits before the per-node geometry recursion when
    /// nothing is dirty. That undersells the cost: `layout_with_ops`
    /// runs a dozen per-frame passes *before* it gets near that
    /// short-circuit — pending animations, the frame tick, the animation
    /// scheduler tick, drag ticks, the whole of `process_state_changes`,
    /// tooltips, delayed / pointer-leave / auto-dismiss overlays and
    /// overlay fades — and it ran all of them for every open window on
    /// every dispatched event, including a fast mouse-move stream.
    ///
    /// `needs_reconcile()` is cheap enough to ask instead (`u64`
    /// comparisons over unique bound sources, no arena walk) and answers
    /// precisely the question this sweep is for. It is safe to skip the
    /// rest because everything in that list has its own scheduling path
    /// — the timing-driven passes all feed `WidgetTree::next_timer_deadline`,
    /// which `request_redraw_due` polls — see `needs_reconcile`'s own doc
    /// for the case-by-case argument.
    ///
    /// The `needs_render()` check stays OUTSIDE the gate on purpose: a
    /// window can need a repaint for reasons that never involved a
    /// binding (a handler called `request_rebuild`, an event dirtied a
    /// node directly), and skipping the reconcile must not also skip
    /// poking it.
    ///
    /// # Reconciling here does not rob the window of its own reconcile
    ///
    /// Load-bearing, and it was not always true. Dirty tracking used to
    /// be a `bool` living on the `Signal`, which the first registry to
    /// flush both read AND cleared — so this very sweep was the thing
    /// that broke shared-`Signal` fan-out: whichever window it happened
    /// to visit first consumed the change and every later window's
    /// reconcile found nothing, permanently. `Signal` now exposes a
    /// monotone generation and each `BindingRegistry` remembers what it
    /// last acted on (see `teksilo_core::binding::BindingGroup`), so
    /// visiting windows in any order — this method iterates a `HashMap`,
    /// so the order is arbitrary and varies run to run — delivers the
    /// change to all of them.
    ///
    /// One known limitation: this uses `WidgetTree::layout` (a
    /// `NoopWindowOps` sink), not `layout_with_ops`, so a handler that
    /// this reconcile pass happens to run (a data-driven rebuild reacting
    /// to the very state change being reconciled) cannot synchronously
    /// open a window from here. That is acceptable for a background
    /// reconciliation pass — the window still opens correctly the next
    /// time this sibling performs its own real redraw with a real
    /// `WindowOps` sink, exactly the way an app that never called this
    /// method at all would have behaved.
    ///
    /// # Why `needs_render()`, not `needs_redraw()`
    ///
    /// Deliberately narrower than the broader `needs_redraw()`, which
    /// also reports `true` while a per-frame shader animation is merely
    /// *running*, with no dirty paint or layout at all. Treating "an
    /// animation is running" as a reason to force an extra redraw here —
    /// on top of whatever other window's event just got dispatched —
    /// would effectively re-couple that animating window's frame rate to
    /// the dispatch rate of every OTHER window's input (a fast mouse-move
    /// stream can exceed 60 Hz), defeating the 60 Hz `WaitUntil` pacing
    /// those animations already get from `next_timer_deadline` /
    /// `request_redraw_due` and reintroducing the exact uncapped
    /// free-running redraw behaviour that pacing was written to remove.
    pub fn request_redraw_needing_render(&mut self) -> usize {
        let now = std::time::Instant::now();
        let mut poked = 0;
        for managed in self.windows.values_mut() {
            if managed.tree.needs_reconcile() {
                let size = managed.platform_window.surface_size();
                let sf = managed.platform_window.scale_factor() as f32;
                let proposal =
                    teksilo_canvas::SizeProposal::exact(size.0 as f32 / sf, size.1 as f32 / sf);
                managed.tree.layout(proposal);
            }
            // A window that draws nothing is skipped: its paint marks wait
            // for the redraw that shows it again, and a request here after
            // each of its ticks would re-arm the next one forever.
            if managed.tree.needs_render() && !managed.redraw.draws_nothing(now) {
                managed.request_redraw();
                poked += 1;
            }
        }
        poked
    }

    /// Drain pending modal requests from all windows.
    pub fn drain_pending_modal_requests(
        &mut self,
    ) -> Vec<(TeksiloWindowId, Vec<teksilo_core::QueuedModalRequest>)> {
        let mut all_requests = Vec::new();
        for managed in self.windows.values_mut() {
            let requests = managed.tree.drain_pending_modal_requests();
            if !requests.is_empty() {
                all_requests.push((managed.teksilo_id, requests));
            }
        }
        all_requests
    }

    /// Drain native modal-window dismiss requests from all windows, and
    /// the application tree's, which closes nothing: its dialogs are
    /// presented in a window's tree or a window of their own, where their
    /// answer asks for the dismissal.
    pub fn drain_pending_modal_dismissals(&mut self) -> Vec<TeksiloWindowId> {
        let mut windows_to_close = Vec::new();
        for managed in self.windows.values_mut() {
            if managed.tree.drain_pending_modal_dismissal()
                && (managed.modal || self.modal_hosts.contains(&managed.teksilo_id))
            {
                windows_to_close.push(managed.teksilo_id);
            }
        }
        if let Some(tree) = &mut self.app_tree
            && tree.drain_pending_modal_dismissal()
        {
            #[cfg(debug_assertions)]
            eprintln!(
                "teksilo-app: dismiss_modal called from the application tree, which presents \
                 no dialog of its own; nothing closed"
            );
        }
        windows_to_close
    }

    /// Drain per-tree close-window requests raised by handlers via
    /// [`EventContext::close_window`](teksilo_core::widget::EventContext::close_window)
    /// (a *guarded* close) and
    /// [`EventContext::close_window_forced`](teksilo_core::widget::EventContext::close_window_forced)
    /// (a *forced* close that bypasses the window's close guard).
    /// Returns `true` when at least one window was queued for closing.
    ///
    /// A forced request wins over a guarded one for the same window in
    /// the same drain — if a handler called both, the window closes
    /// unconditionally.
    pub fn drain_close_window_requests(&mut self) -> bool {
        let mut guarded: Vec<TeksiloWindowId> = Vec::new();
        let mut forced: Vec<TeksiloWindowId> = Vec::new();
        for managed in self.windows.values_mut() {
            // Drain both flags so neither lingers to a later tick.
            let wants_guarded = managed.tree.take_close_window_request();
            let wants_forced = managed.tree.take_force_close_request();
            if wants_forced {
                forced.push(managed.teksilo_id);
            } else if wants_guarded {
                guarded.push(managed.teksilo_id);
            }
        }
        // The application tree is no window: a close it asks for closes
        // nothing. Drained so it does not linger, and said in debug builds.
        if let Some(tree) = &mut self.app_tree {
            let wants_guarded = tree.take_close_window_request();
            let wants_forced = tree.take_force_close_request();
            #[cfg(debug_assertions)]
            if wants_guarded || wants_forced {
                eprintln!(
                    "teksilo-app: close_window called from the application tree, which is no \
                     window's; nothing closed. Close a window by id (close_window_by_id) or \
                     quit the application (quit)."
                );
            }
            #[cfg(not(debug_assertions))]
            let _ = (wants_guarded, wants_forced);
        }
        let any = !guarded.is_empty() || !forced.is_empty();
        for id in forced {
            self.queue_close(id);
        }
        for id in guarded {
            self.request_close(id);
        }
        any
    }

    /// Drain per-tree locale-switch requests raised by handlers via
    /// [`EventContext::set_locale`](teksilo_core::widget::EventContext::set_locale),
    /// parse each one to a `LanguageIdentifier`, and route it through
    /// [`WindowManager::set_locale`] so the `I18nManager` (active locale,
    /// version signal, RTL direction) and every tree stay in sync.
    /// Invalid or unsupported locale strings are logged and dropped.
    ///
    /// Returns `true` if any request was drained, so the caller can repaint
    /// every window — the fan-out marks non-originating windows dirty but
    /// only the window that received the triggering event gets its own
    /// `request_redraw`.
    pub fn drain_pending_locale_requests(&mut self) -> bool {
        let mut requests: Vec<String> = Vec::new();
        for tree in self.trees_mut() {
            if let Some(loc) = tree.take_pending_locale_request() {
                requests.push(loc);
            }
        }
        let had_requests = !requests.is_empty();
        for loc_str in requests {
            match loc_str.parse::<teksilo_i18n::LanguageIdentifier>() {
                Ok(loc) => self.set_locale(loc),
                Err(e) => {
                    eprintln!("teksilo-app: invalid locale `{loc_str}` requested by handler: {e}")
                }
            }
        }
        had_requests
    }

    /// Drain per-tree theme-switch requests raised by handlers via
    /// [`EventContext::set_theme`](teksilo_core::widget::EventContext::set_theme)
    /// and route each through [`WindowManager::set_theme`] so the new theme is
    /// applied to *every* window, not just the one whose handler requested it.
    ///
    /// Returns `true` if any request was drained (same repaint rationale as
    /// [`WindowManager::drain_pending_locale_requests`]). If several windows
    /// raised a request in the same tick, each is applied in turn — the last
    /// wins, matching the locale path.
    pub fn drain_pending_theme_requests(&mut self) -> bool {
        let mut requests: Vec<Theme> = Vec::new();
        for tree in self.trees_mut() {
            if let Some(theme) = tree.take_pending_theme_request() {
                requests.push(theme);
            }
        }
        self.apply_theme_requests(requests)
    }

    /// Set each theme of `requests` in turn, the last winning, then, when
    /// there was one, leave the OS-following modes for
    /// [`ThemeMode::Manual`]; whether there was one. The half of
    /// [`drain_pending_theme_requests`](Self::drain_pending_theme_requests)
    /// after the requests are taken from the windows.
    fn apply_theme_requests(&mut self, requests: Vec<Theme>) -> bool {
        let had_requests = !requests.is_empty();
        for theme in requests {
            self.set_theme(theme);
        }
        if had_requests {
            // An explicit theme pick disables OS-following so a later OS
            // light/dark change won't override the chosen theme. (The
            // internal `apply_os_theme` path sets its themes directly, not
            // through this drain, so it is unaffected.) Nothing is left to
            // listen to the desktop for.
            self.theme_mode = ThemeMode::Manual;
            self.sync_os_theme_watch();
        }
        had_requests
    }

    /// Drain per-tree "follow OS theme" requests raised by handlers (in a
    /// window or in the application tree) via
    /// [`EventContext::follow_system_theme`](teksilo_core::widget::EventContext::follow_system_theme).
    /// Switches the app to [`ThemeMode::Native`] and recomputes the theme from
    /// the current OS colours, fanning it to every window. Returns `true` if
    /// any window requested it (so the caller schedules a repaint).
    pub fn drain_pending_follow_system_requests(&mut self) -> bool {
        let mut requested = false;
        for tree in self.trees_mut() {
            if tree.take_pending_follow_system_request() {
                requested = true;
            }
        }
        if requested {
            self.follow_system_theme(OsQueries::PLATFORM);
        }
        requested
    }

    /// Switch the app to [`ThemeMode::Native`] and apply the theme the
    /// desktop, read through `queries`, resolves to.
    fn follow_system_theme(&mut self, queries: OsQueries) {
        self.theme_mode = ThemeMode::Native;
        // From now on the desktop's changes are heard as they happen.
        self.sync_os_theme_watch();
        // No winit hint at request time; the current window's reported theme
        // stands in for it.
        self.apply_os_theme_from(None, queries);
    }

    /// Drain per-tree text-scale requests raised by handlers via
    /// [`EventContext::set_text_scale`](teksilo_core::widget::EventContext::set_text_scale)
    /// and route each through [`WindowManager::set_text_scale`] so the new
    /// factor is applied to *every* window. Returns `true` if any request was
    /// drained (so the caller schedules a repaint). Last writer wins if several
    /// windows requested in the same tick.
    pub fn drain_pending_text_scale_requests(&mut self) -> bool {
        let mut requests: Vec<f32> = Vec::new();
        for tree in self.trees_mut() {
            if let Some(scale) = tree.take_pending_text_scale_request() {
                requests.push(scale);
            }
        }
        let had_requests = !requests.is_empty();
        for scale in requests {
            self.set_text_scale(scale);
        }
        had_requests
    }
}

/// Translate a [`WindowCommand`] into the appropriate winit call: the new
/// inner size of a resize the window system applied at once (as winit
/// reports it, or as the window's size changed under the call), `None`
/// otherwise.
///
/// `Close` is handled elsewhere (see [`WindowManager::drain_window_commands`]).
fn apply_window_command(
    win: &winit::window::Window,
    cmd: WindowCommand,
) -> Option<winit::dpi::PhysicalSize<u32>> {
    use winit::window::{Fullscreen, UserAttentionType, WindowLevel};
    match cmd {
        WindowCommand::SetPlacement(p) => match p {
            WindowPlacement::Floating => {
                win.set_minimized(false);
                win.set_fullscreen(None);
                win.set_maximized(false);
            }
            WindowPlacement::Maximized => {
                win.set_minimized(false);
                win.set_fullscreen(None);
                win.set_maximized(true);
            }
            WindowPlacement::Fullscreen => {
                win.set_minimized(false);
                win.set_fullscreen(Some(Fullscreen::Borderless(None)));
            }
            WindowPlacement::Minimized => {
                win.set_minimized(true);
            }
        },
        WindowCommand::SetTitle(title) => win.set_title(&title),
        WindowCommand::SetSize(w, h) => {
            // winit says when it applied the size at once on some
            // platforms only (macOS applies it and answers `None`): a size
            // that changed under the request was applied.
            let before = win.inner_size();
            return win
                .request_inner_size(winit::dpi::LogicalSize::new(w, h))
                .or_else(|| Some(win.inner_size()).filter(|now| *now != before));
        }
        WindowCommand::SetPosition(x, y) => {
            win.set_outer_position(winit::dpi::LogicalPosition::new(x, y));
        }
        WindowCommand::SetResizable(r) => win.set_resizable(r),
        WindowCommand::SetAlwaysOnTop(on) => {
            win.set_window_level(if on {
                WindowLevel::AlwaysOnTop
            } else {
                WindowLevel::Normal
            });
        }
        WindowCommand::RequestAttention(kind) => {
            let winit_kind = match kind {
                UserAttentionKind::Critical => UserAttentionType::Critical,
                UserAttentionKind::Informational => UserAttentionType::Informational,
            };
            win.request_user_attention(Some(winit_kind));
        }
        WindowCommand::Focus { activation_token } => {
            raise_window(win, activation_token.as_deref());
        }
        WindowCommand::Close => {
            // Handled in drain_window_commands; unreachable here.
        }
    }
    None
}

/// App-level implementation of [`teksilo_core::WindowOps`] handed into
/// every `dispatch_event_with_ops` call.
///
/// Holds `&mut WindowManager` plus `&ActiveEventLoop` so
/// [`open_window`](teksilo_core::WindowOps::open_window) can create the
/// winit-level window synchronously before returning. Constructed by
/// `TeksiloAppHandler::dispatch_in_window` after temporarily removing
/// the dispatching window from `WindowManager::windows`; the removed
/// tree is borrowed mutably for the handler run.
pub struct WindowOpsImpl<'a> {
    wm: &'a mut WindowManager,
    event_loop: &'a winit::event_loop::ActiveEventLoop,
    /// Current (dispatching) window's id. Names the window the outbound
    /// OS-drag ops below (`begin_os_drag` / `cancel_os_drag` /
    /// `set_drop_accepted`) route to, and the window a dialog `modal_to` it
    /// is attached to (with `current_window_arc`). `None` for the
    /// [windowless](Self::windowless) sink, which has no window to drag
    /// from.
    current_id: Option<TeksiloWindowId>,
    /// `Arc<Window>` of the current (dispatching) window. A dialog opened
    /// `modal_to` it finds its parent here (owner window, transient-for or
    /// `addChildWindow:ordered:`). It also backs `current_parent_handle()` so native-dialog
    /// integrations can extract both window and display handles even
    /// while the dispatching window is temporarily out of
    /// `WindowManager::windows`.
    current_window_arc: Option<std::sync::Arc<winit::window::Window>>,
}

impl<'a> WindowOpsImpl<'a> {
    pub fn new(
        wm: &'a mut WindowManager,
        event_loop: &'a winit::event_loop::ActiveEventLoop,
        current_id: TeksiloWindowId,
        current_window_arc: Option<std::sync::Arc<winit::window::Window>>,
    ) -> Self {
        Self {
            wm,
            event_loop,
            current_id: Some(current_id),
            current_window_arc,
        }
    }

    /// The sink for code that runs in no window: the application tree's
    /// handlers. Every operation that names a window by id works as from a
    /// window; those about the dispatching window itself find none: no
    /// parent handle (a file dialog refuses), no IME area, no OS drag
    /// (`begin_os_drag` declines), and no activation token for itself.
    pub fn windowless(
        wm: &'a mut WindowManager,
        event_loop: &'a winit::event_loop::ActiveEventLoop,
    ) -> Self {
        Self {
            wm,
            event_loop,
            current_id: None,
            current_window_arc: None,
        }
    }
}

impl teksilo_core::WindowOps for WindowOpsImpl<'_> {
    fn open_window(&mut self, config: teksilo_core::WindowConfig) -> TeksiloWindowId {
        let dispatching = self.current_id.zip(self.current_window_arc.as_deref());
        self.wm
            .create_window_beside(config, self.event_loop, dispatching)
    }

    fn find_window(&self, string_id: &str) -> Option<TeksiloWindowId> {
        self.wm.find_window(string_id)
    }

    fn window_state(&self, id: TeksiloWindowId) -> Option<teksilo_core::WindowState> {
        let winit_id = self.wm.teksilo_to_winit_map().get(&id).copied()?;
        self.wm
            .windows_map()
            .get(&winit_id)
            .map(|m| m.state.clone())
    }

    fn windows(&self) -> Vec<teksilo_core::WindowState> {
        self.wm
            .windows_map()
            .values()
            .map(|m| m.state.clone())
            .collect()
    }

    fn focus_window(&mut self, id: TeksiloWindowId) {
        // The dispatching window is out of the map while its handler runs,
        // and raising it is the common case (a single-instance handler
        // routed to the document window it was asked to show): its own
        // handle is kept for that.
        // A minimised window comes back first: winit's `focus_window`
        // leaves it in the Dock or the taskbar, and on macOS does not even
        // activate the application.
        let window = if self.current_id == Some(id) {
            let window = self.current_window_arc.clone();
            if let Some(window) = &window
                && (self.wm.knows_minimised(id) || window.is_minimized() == Some(true))
            {
                window.set_minimized(false);
                self.wm.note_taken_out_restored(id);
            }
            window
        } else {
            let managed = self
                .wm
                .teksilo_to_winit_map()
                .get(&id)
                .copied()
                .and_then(|winit_id| self.wm.get_by_winit_mut(winit_id));
            managed.map(|managed| {
                managed.restore();
                managed.platform_window.window_arc()
            })
        };
        let Some(window) = window else {
            return;
        };
        // The routed event's activation token, if it carried one: on
        // Wayland the raise is otherwise only an attention request.
        let token = self.wm.use_dispatch_activation_token();
        raise_window(&window, token.as_deref());
    }

    fn request_activation_token(
        &mut self,
        id: TeksiloWindowId,
        cb: Box<dyn FnOnce(Option<String>)>,
    ) {
        let Some(winit_id) = self.wm.teksilo_to_winit_map().get(&id).copied() else {
            cb(None);
            return;
        };
        // Immutable borrow to issue the request, released before we mutate the
        // callback map below.
        let issued = match self.wm.windows_map().get(&winit_id) {
            Some(managed) => teksilo_platform::window_activation::request_activation_token(
                managed.platform_window.window(),
            ),
            None => false,
        };
        if issued {
            self.wm.store_activation_token_callback(winit_id, cb);
        } else {
            cb(None);
        }
    }

    fn request_activation_token_self(&mut self, cb: Box<dyn FnOnce(Option<String>)>) {
        // The current window is temporarily out of `wm.windows` during its own
        // dispatch, so use the captured Arc rather than an id lookup.
        let Some(arc) = self.current_window_arc.clone() else {
            cb(None);
            return;
        };
        if teksilo_platform::window_activation::request_activation_token(&arc) {
            self.wm.store_activation_token_callback(arc.id(), cb);
        } else {
            cb(None);
        }
    }

    fn quit(&mut self) {
        self.wm.quit();
    }

    fn quit_forced(&mut self) {
        self.wm.quit_forced();
    }

    fn quit_in_progress(&self) -> bool {
        self.wm.quit_in_progress()
    }

    fn quit_source(&self) -> Option<teksilo_core::QuitSource> {
        self.wm.quit_source()
    }

    fn cancel_quit(&mut self) {
        self.wm.cancel_quit();
    }

    fn take_activation_token(&mut self) -> Option<String> {
        self.wm.take_dispatch_activation_token()
    }

    fn hold_activation_token(&mut self, token: String) {
        self.wm.set_dispatch_activation_token(token);
    }

    fn release_activation_token(&mut self) {
        self.wm.end_dispatch_activation();
    }

    fn close_window_by_id(&mut self, id: TeksiloWindowId) {
        self.wm.queue_close(id);
    }

    fn current_parent_handle(&self) -> Option<teksilo_core::raw_handle::ParentHandle> {
        // Always extract from `current_window_arc` because the
        // dispatching window is temporarily out of `wm.windows_map()`
        // during event delivery.
        let arc = self.current_window_arc.as_ref()?;
        teksilo_core::raw_handle::ParentHandle::from_window(arc.as_ref())
    }

    fn soft_keyboard_support(&self) -> teksilo_core::window::SoftKeyboardSupport {
        // The host's row, not the trait's `None` default. Without this
        // override every widget asking whether a keyboard can be raised is
        // told no on every platform — including the one whose row is
        // `Explicit` — and offers the wrong affordance. Named there rather
        // than written here so a test can assert against the same thing;
        // `host_soft_keyboard_support`'s own doc says what such a test can
        // and cannot catch.
        crate::input_loop::host_soft_keyboard_support()
    }

    fn set_ime_cursor_area(&mut self, area: teksilo_canvas::Rect) {
        // Applied directly to the in-flight window (out of `wm.windows_map()`
        // during dispatch). Repositioning the candidate area is idempotent —
        // unlike `set_ime_allowed`, it never cancels an active composition —
        // so no dedup is needed; text widgets only report it on caret moves.
        if let Some(arc) = self.current_window_arc.as_ref() {
            arc.set_ime_cursor_area(
                winit::dpi::LogicalPosition::new(area.x, area.y),
                winit::dpi::LogicalSize::new(area.width.max(1.0), area.height.max(1.0)),
            );
        }
    }

    fn begin_os_drag(
        &mut self,
        data: teksilo_core::OutboundDragData,
        image: Option<teksilo_core::DragImageData>,
        pointer: teksilo_tokens::PointerKind,
    ) -> bool {
        use teksilo_platform::external_dnd::ExternalDndHandle;
        // Outbound drag is wired only if the app installed the external-DnD
        // service. Without it, decline so the framework keeps the in-app drag
        // alive.
        let Some(handle) = self
            .wm
            .app_context_template()
            .and_then(|t| t.app_state::<ExternalDndHandle>().cloned())
        else {
            return false;
        };
        let Some(current_id) = self.current_id else {
            return false;
        };
        handle.begin_drag(current_id, &data, image.as_ref(), pointer)
    }

    fn cancel_os_drag(&mut self) {
        use teksilo_platform::external_dnd::ExternalDndHandle;
        if let Some(handle) = self
            .wm
            .app_context_template()
            .and_then(|t| t.app_state::<ExternalDndHandle>().cloned())
            && let Some(current_id) = self.current_id
        {
            handle.cancel_drag(current_id);
        }
    }

    fn set_drop_accepted(&mut self, accepted: bool) {
        use teksilo_platform::external_dnd::ExternalDndHandle;
        if let Some(handle) = self
            .wm
            .app_context_template()
            .and_then(|t| t.app_state::<ExternalDndHandle>().cloned())
            && let Some(current_id) = self.current_id
        {
            handle.set_drop_accepted(current_id, accepted);
        }
    }
}

/// Raise `window`, activated with `token` where the window system needs one
/// (see [`window_activation::raise`](teksilo_platform::window_activation::raise)).
/// Every raise teksilo-app makes goes through here; a test build records it
/// (`activation_log`).
pub(crate) fn raise_window(window: &winit::window::Window, token: Option<&str>) {
    #[cfg(test)]
    activation_log::note(window.id(), activation_log::Activation::Raised);
    teksilo_platform::window_activation::raise(window, token);
}

/// Ask the window system to draw the user's attention to `window`, for a
/// dialog waiting on the user, without raising it: critical, as AppKit asks
/// for a modal panel of an application in the background, so the Dock icon
/// bounces and the window and its taskbar button flash on Windows until the
/// user comes to the application; an urgency hint on X11. A test build
/// records it (`activation_log`).
pub(crate) fn request_window_attention(window: &winit::window::Window) {
    #[cfg(test)]
    activation_log::note(window.id(), activation_log::Activation::Attention);
    window.request_user_attention(Some(winit::window::UserAttentionType::Critical));
}

/// Withdraw the attention [`request_window_attention`] asked for `window`:
/// the user came to it. On X11 this clears the urgency hint, which
/// otherwise stays set; macOS and Windows end the request themselves as the
/// window comes to the front. A test build records it (`activation_log`).
pub(crate) fn clear_window_attention(window: &winit::window::Window) {
    #[cfg(test)]
    activation_log::note(window.id(), activation_log::Activation::AttentionCleared);
    window.request_user_attention(None);
}

/// What a test build saw teksilo-app ask of the window system to bring a
/// window to the user: the raises and attention requests, oldest first, on
/// the thread that made them (the event loop's).
#[cfg(test)]
pub(crate) mod activation_log {
    use std::cell::RefCell;

    /// One request.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Activation {
        /// [`raise_window`](super::raise_window).
        Raised,
        /// [`request_window_attention`](super::request_window_attention).
        Attention,
        /// A new window shown without taking the focus: a dialog opened
        /// quietly ([`AppDialogPresentation::Quiet`](super::AppDialogPresentation::Quiet)).
        ShownInactive,
        /// [`clear_window_attention`](super::clear_window_attention): the
        /// window, or a dialog of it, took the focus, so the attention it
        /// asked for ends.
        AttentionCleared,
    }

    thread_local! {
        static LOG: RefCell<Vec<(winit::window::WindowId, Activation)>> =
            const { RefCell::new(Vec::new()) };
    }

    pub(crate) fn note(window: winit::window::WindowId, what: Activation) {
        LOG.with(|log| log.borrow_mut().push((window, what)));
    }

    /// Take everything recorded so far. Read by the winit loop tests, which
    /// only a Linux test build has.
    #[cfg(target_os = "linux")]
    pub(crate) fn take() -> Vec<(winit::window::WindowId, Activation)> {
        LOG.with(|log| std::mem::take(&mut *log.borrow_mut()))
    }
}

/// What a modal dialog does when the window it blocks (its owner) is
/// minimised or restored; see [`follow_owner`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FollowOwner {
    /// Nothing: the owner did not change, or the OS keeps the dialog with it.
    Nothing,
    /// The owner was minimised: minimise the dialog with it.
    Minimise,
    /// The owner was restored: restore the dialog that went with it, and
    /// raise it.
    Restore,
}

/// Whether the window system minimises and restores a dialog with the
/// window it blocks by itself: Windows does with an owned window (it hides
/// it with its owner and shows it again), macOS with a child window. X11
/// leaves it to the window manager, which may or may not, and Wayland has no
/// parent relationship in winit at all.
pub(crate) const OS_KEEPS_DIALOGS_WITH_OWNER: bool =
    cfg!(any(target_os = "windows", target_os = "macos"));

/// The rule behind [`WindowManager::follow_owners`]: a dialog follows its
/// owner's minimise and restore, where the window system does not do it
/// (`os_keeps` false). It follows the owner's *changes*, from `was` (the
/// owner's minimised state last seen) to `now`, so a dialog the user brings
/// back on its own is not minimised again. Only a dialog that went with its
/// owner (`went_with`) is restored with it.
///
/// A dialog has no minimise of its own (`WindowConfig::minimizable` is
/// ignored for a modal window), so nothing goes the other way.
pub(crate) fn follow_owner(was: bool, now: bool, went_with: bool, os_keeps: bool) -> FollowOwner {
    if os_keeps || was == now {
        return FollowOwner::Nothing;
    }
    if now {
        FollowOwner::Minimise
    } else if went_with {
        FollowOwner::Restore
    } else {
        FollowOwner::Nothing
    }
}

/// Answer a dialog request never shown as dismissed, in `tree`, the tree
/// that asked for it: its `on_dismiss` runs with
/// [`DismissReason::Programmatic`](teksilo_core::overlay::DismissReason::Programmatic),
/// as for a dialog whose window closes, so a `MessageBox` reports its Escape
/// button (Cancel). Its content is built first, in a tree nothing shows,
/// since a dialog answers from the state its content holds.
fn answer_unshown_dialog(
    request: teksilo_core::modal::ModalRequest,
    tree: &mut WidgetTree,
    ops: &mut WindowOpsImpl,
) {
    let Some(on_dismiss) = request.on_dismiss else {
        return;
    };
    if let teksilo_core::modal::ModalContent::Deferred(builder) = request.content {
        let mut unshown = WidgetTree::new();
        builder(&mut unshown);
    }
    let reason = teksilo_core::overlay::DismissReason::Programmatic;
    tree.run_with_event_context(ops, |ctx| on_dismiss(reason, ctx));
}

/// Where a window whose content is `size` wide and high (logical) goes to
/// be centred on `centre`: its frame's corner, rounded to the logical
/// pixel, the corner every position teksilo-app asks for names. `insets`
/// is what the frame adds to the content on each axis (logical, see
/// [`frame_insets`]).
pub(crate) fn centred_on(centre: (f64, f64), size: (u32, u32), insets: (f64, f64)) -> (i32, i32) {
    (
        (centre.0 - (size.0 as f64 + insets.0) / 2.0).round() as i32,
        (centre.1 - (size.1 as f64 + insets.1) / 2.0).round() as i32,
    )
}

/// What `window`'s frame adds to its content on each axis, logical: its
/// outer size less its inner size, both as the window system reports them
/// (nothing on Wayland, where the compositor draws or the client does).
pub(crate) fn frame_insets(window: &winit::window::Window) -> (f64, f64) {
    let sf = window.scale_factor();
    let (outer, inner) = (window.outer_size(), window.inner_size());
    (
        outer.width.saturating_sub(inner.width) as f64 / sf,
        outer.height.saturating_sub(inner.height) as f64 / sf,
    )
}

#[cfg(test)]
mod centred_on_tests {
    use super::centred_on;

    /// The frame is centred, not the content: a 28 dp macOS title bar puts
    /// the corner 14 dp higher than the content alone would, and a frame's
    /// borders widen it.
    #[test]
    fn the_frame_is_centred_not_the_content() {
        let centre = (500.0, 400.0);
        assert_eq!(centred_on(centre, (300, 200), (0.0, 0.0)), (350, 300));
        assert_eq!(centred_on(centre, (300, 200), (0.0, 28.0)), (350, 286));
        assert_eq!(centred_on(centre, (300, 200), (16.0, 39.0)), (342, 281));
    }
}

/// What [`WindowManager::take_managed`] remembers of a window it takes out
/// of the map while a handler runs in it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TakenOut {
    focused: bool,
    minimized: bool,
}

/// How the application tree's dialog, or a window's own, is presented over
/// the window it goes to; see [`app_dialog_presentation`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppDialogPresentation {
    /// Shown and brought to the front (a window's own in its tree is shown
    /// there, its window left as it is); for the application tree's, the
    /// window it goes to is restored first if minimised.
    Raise,
    /// Shown where it is, without taking the focus; the window it goes to
    /// asks for the user's attention, once for the dialog.
    Quiet,
    /// Not shown yet: kept until the user comes back (a window of the
    /// application takes the focus, or, for a window's own dialog, that
    /// window), the application asking for attention meanwhile.
    Owe,
}

/// How a dialog the application tree asked for, or a window's own, is
/// presented over `target`: raised when it comes from the user's input
/// (`for_user`) or `target` or a dialog of it has the focus
/// (`target_focused`, see [`WindowManager::knows_focused_with_dialogs`]: the
/// application is in front, and raising takes the focus from no other);
/// otherwise shown quietly, the window asking for attention, unless it
/// would open a window of its own (`own_window`) over a minimised `target`:
/// such a window would be drawn without the window it belongs to (AppKit
/// orders a child window in over its miniaturised parent), so it is owed
/// until the user comes back. A dialog in `target`'s own tree waits there,
/// minimised with it.
pub(crate) fn app_dialog_presentation(
    for_user: bool,
    target_focused: bool,
    target_minimised: bool,
    own_window: bool,
) -> AppDialogPresentation {
    if for_user || target_focused {
        AppDialogPresentation::Raise
    } else if target_minimised && own_window {
        AppDialogPresentation::Owe
    } else {
        AppDialogPresentation::Quiet
    }
}

/// What [`WindowManager::follow_owners`] remembers of one dialog's owner.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct OwnerSeen {
    /// The owner was minimised when last looked at.
    minimised: bool,
    /// The dialog was asked to minimise because its owner was (whether the
    /// window manager did or not), so it is restored with it.
    went_with: bool,
}

/// What a dialog whose raise did not take tells the manager about itself;
/// see [`owed_raise`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DialogSign {
    /// It is shown again: winit's `Occluded(false)`.
    Shown,
    /// It took the focus.
    Focused,
    /// It is asked to draw a frame.
    Redrawn,
}

/// What to do with a raise a dialog is owed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OwedRaise {
    /// Not yet: it is still hidden.
    Keep,
    /// Raise it now, and owe nothing more.
    Raise,
    /// Owe nothing more: it is in front already.
    Forget,
}

/// The rule behind [`WindowManager::note_dialog_sign`].
///
/// A dialog is raised when the window it blocks takes the focus. On macOS
/// the window it blocks may have just come back from the Dock: AppKit hid
/// the dialog, a child window, with it, and shows it again only once the
/// restore animation ends. winit's `focus_window` does nothing for a window
/// that is not visible, so that raise is lost, the window it blocks stays
/// key, and the first click or key there is swallowed. The raise is owed
/// instead, and paid on the first sign that the dialog is on screen again:
/// winit's `Occluded(false)`, or a frame while the window system reports
/// it visible. A dialog that takes the focus by itself owes nothing.
/// `visible` is winit's `is_visible`, `None` where it cannot tell.
pub(crate) fn owed_raise(sign: DialogSign, visible: Option<bool>) -> OwedRaise {
    match sign {
        DialogSign::Focused => OwedRaise::Forget,
        DialogSign::Shown => OwedRaise::Raise,
        DialogSign::Redrawn if visible == Some(true) => OwedRaise::Raise,
        DialogSign::Redrawn => OwedRaise::Keep,
    }
}

#[cfg(test)]
mod owed_raise_tests {
    use super::*;

    /// macOS item 5: restored from the Dock, the window took the focus while
    /// AppKit still hid its dialog; the dialog's `Occluded(false)` at the end
    /// of the animation is when it can be raised.
    #[test]
    fn a_dialog_shown_again_is_raised() {
        assert_eq!(owed_raise(DialogSign::Shown, Some(true)), OwedRaise::Raise);
        assert_eq!(owed_raise(DialogSign::Shown, None), OwedRaise::Raise);
    }

    #[test]
    fn a_frame_raises_it_only_once_it_is_visible() {
        assert_eq!(
            owed_raise(DialogSign::Redrawn, Some(false)),
            OwedRaise::Keep
        );
        assert_eq!(owed_raise(DialogSign::Redrawn, None), OwedRaise::Keep);
        assert_eq!(
            owed_raise(DialogSign::Redrawn, Some(true)),
            OwedRaise::Raise
        );
    }

    #[test]
    fn a_dialog_that_took_the_focus_owes_nothing() {
        assert_eq!(
            owed_raise(DialogSign::Focused, Some(false)),
            OwedRaise::Forget
        );
    }
}

#[cfg(test)]
mod follow_owner_tests {
    use super::*;

    #[test]
    fn a_dialog_is_minimised_with_its_owner() {
        assert_eq!(
            follow_owner(false, true, false, false),
            FollowOwner::Minimise
        );
    }

    #[test]
    fn a_dialog_that_went_with_its_owner_comes_back_with_it() {
        assert_eq!(follow_owner(true, false, true, false), FollowOwner::Restore);
    }

    /// The owner coming back restores no dialog Teksilo did not minimise:
    /// the window manager minimised it, or the user did.
    #[test]
    fn a_dialog_that_did_not_go_with_its_owner_is_left_alone() {
        assert_eq!(
            follow_owner(true, false, false, false),
            FollowOwner::Nothing
        );
    }

    /// Only a change of the owner counts: a dialog the user restores while
    /// its owner stays minimised is not minimised again.
    #[test]
    fn an_owner_that_stays_minimised_changes_nothing() {
        assert_eq!(follow_owner(true, true, true, false), FollowOwner::Nothing);
        assert_eq!(
            follow_owner(false, false, true, false),
            FollowOwner::Nothing
        );
    }

    /// Windows and macOS hide and show an owned or child window with its
    /// owner themselves; minimising it as well would leave it iconic on its
    /// own.
    #[test]
    fn where_the_os_keeps_the_dialog_with_its_owner_nothing_is_done() {
        assert_eq!(follow_owner(false, true, false, true), FollowOwner::Nothing);
        assert_eq!(follow_owner(true, false, true, true), FollowOwner::Nothing);
    }
}

#[cfg(test)]
mod app_dialog_presentation_tests {
    use super::AppDialogPresentation::{Owe, Quiet, Raise};
    use super::app_dialog_presentation;

    /// Every case: (for the user, target focused, target minimised, in a
    /// window of its own) and the answer. Only the user's request or a
    /// focused target raises; only a window of its own over a minimised
    /// target is owed; a dialog in the target's own tree waits there.
    #[test]
    fn the_application_trees_dialog_takes_only_the_focus_it_is_given() {
        let table = [
            ((true, false, false, true), Raise),
            ((true, false, true, true), Raise),
            ((true, true, false, false), Raise),
            ((false, true, false, true), Raise),
            ((false, false, false, true), Quiet),
            ((false, false, false, false), Quiet),
            ((false, false, true, false), Quiet),
            ((false, false, true, true), Owe),
        ];
        for ((for_user, focused, minimised, own_window), expected) in table {
            assert_eq!(
                app_dialog_presentation(for_user, focused, minimised, own_window),
                expected,
                "for_user {for_user}, focused {focused}, minimised {minimised}, \
                 own window {own_window}"
            );
        }
    }
}

#[cfg(test)]
mod dialog_close_tests {
    use super::*;

    fn pair() -> (WindowManager, TeksiloWindowId, TeksiloWindowId) {
        let mut wm = WindowManager::new(teksilo_core::presets::intui::light());
        let (owner, dialog) = (TeksiloWindowId::new(1), TeksiloWindowId::new(2));
        wm.modal_blocked.insert(owner, dialog);
        wm.modal_parents.insert(dialog, owner);
        (wm, owner, dialog)
    }

    /// A close request on a window its dialog blocks is refused, as a
    /// disabled owner refuses it on Windows; one queued for the dialog
    /// first lets it through.
    #[test]
    fn a_window_its_dialog_blocks_refuses_a_close_request() {
        let (mut wm, owner, dialog) = pair();
        wm.request_close(owner);
        assert!(!wm.is_closing(owner));
        wm.queue_close(dialog);
        assert_eq!(
            wm.blocking_modal_child(owner),
            None,
            "a dialog about to close blocks nothing"
        );
        wm.request_close(owner);
        assert!(wm.is_closing(owner));
    }

    /// Closing the owner closes its dialogs: none is left pointing at it.
    #[test]
    fn closing_an_owner_closes_its_dialogs() {
        let (mut wm, owner, dialog) = pair();
        wm.close_window(owner);
        assert!(!wm.modal_parents.contains_key(&dialog));
        assert!(!wm.is_blocked(owner));
    }
}

#[cfg(test)]
mod caption_button_tests {
    use super::*;
    use winit::window::WindowButtons;

    #[test]
    fn a_dialog_gets_only_a_close_button() {
        let parent = TeksiloWindowId::new(1);
        assert_eq!(
            caption_buttons(&WindowConfig::new().modal_to(parent)),
            WindowButtons::CLOSE
        );
        assert_eq!(caption_buttons(&WindowConfig::new()), WindowButtons::all());
        assert_eq!(
            caption_buttons(&WindowConfig::new().minimizable(false)),
            WindowButtons::CLOSE | WindowButtons::MAXIMIZE
        );
        // A dialog asking for a minimise button gets none: it follows the
        // window it blocks.
        assert_eq!(
            caption_buttons(
                &WindowConfig::new()
                    .modal_to(parent)
                    .minimizable(true)
                    .maximizable(true)
            ),
            WindowButtons::CLOSE | WindowButtons::MAXIMIZE
        );
    }
}

#[cfg(test)]
mod wrap_root_tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;
    use teksilo_canvas::SizeProposal;
    use teksilo_core::build_context::BuildContext;
    use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
    use teksilo_core::{Action, Intent};
    use teksilo_widgets::primitives::VStack;

    /// An app root answering `test.open` with a scoped action, as the
    /// native-menu example's root does.
    #[derive(Debug)]
    struct AppRoot(Rc<Cell<u32>>);

    impl Widget for AppRoot {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let runs = self.0.clone();
            ctx.register_action(Action::new("test.open").on_invoke(move |_, _| {
                runs.set(runs.get() + 1);
            }));
            vec![]
        }

        fn layout_response(&self, proposal: SizeProposal, _: &LayoutContext) -> LayoutResponse {
            proposal.resolve(10.0, 10.0).into()
        }
    }

    /// A post-root hook wrapping the root in a container, as the debug
    /// inspector's does.
    fn wrapper() -> crate::DefaultPostRoot {
        crate::DefaultPostRoot::new(|tree, root| tree.add(VStack::new().child(root)))
    }

    fn open_from_a_callback(post_root: Option<crate::DefaultPostRoot>) -> u32 {
        let runs = Rc::new(Cell::new(0));
        let mut tree = WidgetTree::new();
        let root = tree.add(AppRoot(runs.clone()));
        let wrapped = wrap_root(&mut tree, root, None, post_root);
        tree.layout(SizeProposal::exact(400.0, 300.0));
        assert_eq!(tree.roots().first(), Some(&wrapped));
        tree.run_with_event_context(&mut teksilo_core::window::NoopWindowOps, |ctx| {
            ctx.send_intent(Intent::new("test.open"))
        });
        runs.get()
    }

    #[test]
    fn a_callback_reaches_the_app_root_through_a_post_root_wrapper() {
        assert_eq!(open_from_a_callback(None), 1);
        assert_eq!(open_from_a_callback(Some(wrapper())), 1);
    }

    #[test]
    fn a_window_post_root_wins_over_the_app_default() {
        let runs = Rc::new(Cell::new(0));
        let mut tree = WidgetTree::new();
        let root = tree.add(AppRoot(runs.clone()));
        let own: teksilo_core::window::PostRootBuilder =
            Box::new(|tree, root| tree.add(VStack::new().child(root)));
        let default_ran = Rc::new(Cell::new(false));
        let ran = default_ran.clone();
        let default = crate::DefaultPostRoot::new(move |_, root| {
            ran.set(true);
            root
        });
        let wrapped = wrap_root(&mut tree, root, Some(own), Some(default));
        assert_ne!(wrapped, root);
        assert!(!default_ran.get());
        // The app's root is still where a callback's intent starts.
        tree.layout(SizeProposal::exact(400.0, 300.0));
        tree.run_with_event_context(&mut teksilo_core::window::NoopWindowOps, |ctx| {
            ctx.send_intent(Intent::new("test.open"))
        });
        assert_eq!(runs.get(), 1);
    }
}

#[cfg(test)]
mod initial_focus_tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;
    use teksilo_canvas::SizeProposal;
    use teksilo_core::build_context::BuildContext;
    use teksilo_core::widget::Widget;
    use teksilo_i18n::lit;
    use teksilo_widgets::Button;
    use teksilo_widgets::primitives::VStack;

    /// A root holding two focusable buttons, optionally pointing its
    /// `initial_focus_hint` at the SECOND one — so a passing test cannot be
    /// explained by "it happened to pick the first focusable anyway".
    /// `second_out` republishes that id to the test.
    #[derive(Debug)]
    struct Root {
        hint_to_second: bool,
        second: Option<WidgetId>,
        second_out: Rc<Cell<Option<WidgetId>>>,
        root: Option<WidgetId>,
    }

    impl Widget for Root {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            let first = ctx.add(Button::new(lit!("First")));
            let second = ctx.add(Button::new(lit!("Second")));
            self.second = Some(second);
            self.second_out.set(Some(second));
            let col = ctx.add(VStack::new().child(first).child(second));
            self.root = Some(col);
            vec![col]
        }

        fn initial_focus_hint(&self) -> Option<WidgetId> {
            if self.hint_to_second {
                self.second
            } else {
                None
            }
        }

        fn layout_response(
            &self,
            proposal: SizeProposal,
            ctx: &teksilo_core::LayoutContext,
        ) -> teksilo_core::widget::LayoutResponse {
            self.root
                .and_then(|id| ctx.child_size(id, proposal))
                .unwrap_or_else(|| proposal.resolve(0.0, 0.0))
                .into()
        }
    }

    /// Build a headless tree whose root is a `Root`, and return the policy's
    /// choice plus the id it should (or should not) have picked.
    fn focus_for(hint: bool, is_modal: bool) -> (Option<WidgetId>, Option<WidgetId>) {
        let second_out: Rc<Cell<Option<WidgetId>>> = Rc::new(Cell::new(None));
        let mut tree = WidgetTree::new();
        let root = tree.add(Root {
            hint_to_second: hint,
            second: None,
            second_out: second_out.clone(),
            root: None,
        });
        tree.layout(SizeProposal::exact(400.0, 300.0));
        (
            initial_window_focus(&tree, root, is_modal, None),
            second_out.get(),
        )
    }

    #[test]
    fn a_plain_window_honours_an_explicit_focus_hint() {
        // The feature: a window root can direct its own initial focus (the
        // Launcher points at its recents list so Enter opens the highlighted
        // project without a Tab). Must land on the SECOND button — the hint —
        // not merely on the first focusable.
        let (focused, second) = focus_for(true, false);
        assert!(
            second.is_some(),
            "precondition: the root exposes a hint target"
        );
        assert_eq!(
            focused, second,
            "a plain window must focus the widget its root's initial_focus_hint names"
        );
    }

    #[test]
    fn a_plain_window_without_a_hint_focuses_nothing() {
        // The guard rail: NO `first_focusable_descendant` fallback for plain
        // windows. Falling back would silently drop the caret into whatever
        // control comes first in tree order — every existing app window would
        // change behavior.
        let (focused, _) = focus_for(false, false);
        assert_eq!(
            focused, None,
            "a plain window with no hint must open with nothing focused, \
             not steal focus onto its first focusable widget"
        );
    }

    #[test]
    fn a_modal_still_falls_back_to_its_first_focusable() {
        // Unchanged modal policy: a dialog always focuses something, so it can
        // be answered from the keyboard immediately.
        let (focused, second) = focus_for(false, true);
        assert!(
            focused.is_some() && focused != second,
            "a modal with no hint falls back to its FIRST focusable (not the second)"
        );
    }

    #[test]
    fn a_modal_prefers_its_hint_over_the_first_focusable() {
        let (focused, second) = focus_for(true, true);
        assert_eq!(
            focused, second,
            "a modal's hint outranks the first focusable"
        );
    }
}

#[cfg(test)]
mod close_guard_tests {
    use super::*;
    use std::cell::Cell;
    use teksilo_core::signal::Signal;
    use teksilo_core::widget::EventContext;
    use teksilo_core::{CloseResponse, NoopWindowOps, WidgetTree};

    /// Drive `close_verdict` inside a real (headless) `EventContext` and
    /// return whether it lets the close through. Mirrors the
    /// `run_with_event_context` path `process_pending` uses, but with a
    /// `NoopWindowOps` sink and no winit event loop.
    fn verdict(
        can_close: Option<Prop<bool>>,
        on_blocked: Option<CloseBlockedCallback>,
        guard: Option<CloseGuard>,
    ) -> bool {
        matches!(response(can_close, on_blocked, guard), CloseResponse::Close)
    }

    fn response(
        can_close: Option<Prop<bool>>,
        on_blocked: Option<CloseBlockedCallback>,
        guard: Option<CloseGuard>,
    ) -> CloseResponse {
        let mut tree = WidgetTree::new();
        let mut out = CloseResponse::Close;
        tree.run_with_event_context(&mut NoopWindowOps, |ctx: &mut EventContext| {
            out = close_verdict(&can_close, &on_blocked, &guard, ctx);
        });
        out
    }

    #[test]
    fn no_guard_closes() {
        assert!(verdict(None, None, None));
    }

    #[test]
    fn guard_close_proceeds() {
        let guard: CloseGuard = Rc::new(|_ctx| CloseResponse::Close);
        assert!(verdict(None, None, Some(guard)));
    }

    #[test]
    fn guard_veto_keeps_window_open() {
        let guard: CloseGuard = Rc::new(|_ctx| CloseResponse::Veto);
        assert!(!verdict(None, None, Some(guard)));
    }

    /// The guard's deferred answer reaches the manager with the very
    /// decision the guard handed out.
    #[test]
    fn guard_defer_hands_its_decision_over() {
        let decision = teksilo_core::CloseDecision::new();
        let handed = decision.clone();
        let guard: CloseGuard = Rc::new(move |_ctx| CloseResponse::Defer(handed.clone()));
        let CloseResponse::Defer(received) = response(None, None, Some(guard)) else {
            panic!("a deferring guard defers");
        };
        decision.proceed();
        assert_eq!(
            received.receipt().answer(),
            Some(teksilo_core::window::CloseAnswer::Proceed)
        );
    }

    #[test]
    fn can_close_false_vetoes_and_fires_blocked() {
        let fired = Rc::new(Cell::new(false));
        let flag = fired.clone();
        let on_blocked: CloseBlockedCallback = Rc::new(move |_ctx, decision| {
            flag.set(true);
            decision.cancel();
        });

        let should_close = verdict(Some(Prop::from(Signal::new(false))), Some(on_blocked), None);

        assert!(!should_close, "can_close == false must not close");
        assert!(fired.get(), "on_close_blocked must fire on a vetoed close");
    }

    /// Without `on_close_blocked` a blocked close is a plain veto, final
    /// for a quit too.
    #[test]
    fn can_close_false_without_on_blocked_vetoes() {
        let out = response(Some(Prop::from(Signal::new(false))), None, None);
        assert!(matches!(out, CloseResponse::Veto));
    }

    /// With `on_close_blocked` the blocked close defers, on the decision
    /// the callback was handed: its answer is the close's answer.
    #[test]
    fn can_close_false_with_on_blocked_defers_on_the_callbacks_decision() {
        let on_blocked: CloseBlockedCallback = Rc::new(|_ctx, decision| decision.proceed());
        let out = response(Some(Prop::from(Signal::new(false))), Some(on_blocked), None);
        let CloseResponse::Defer(decision) = out else {
            panic!("a blocked close with a callback defers");
        };
        assert_eq!(
            decision.receipt().answer(),
            Some(teksilo_core::window::CloseAnswer::Proceed)
        );
    }

    #[test]
    fn can_close_false_does_not_consult_guard() {
        // The guard would close, but the sugar signal short-circuits to a
        // veto before the guard is ever consulted.
        let guard_ran = Rc::new(Cell::new(false));
        let gflag = guard_ran.clone();
        let guard: CloseGuard = Rc::new(move |_ctx| {
            gflag.set(true);
            CloseResponse::Close
        });

        let should_close = verdict(Some(Prop::from(Signal::new(false))), None, Some(guard));

        assert!(!should_close, "can_close == false wins over the guard");
        assert!(
            !guard_ran.get(),
            "the guard must not run once can_close has vetoed"
        );
    }

    #[test]
    fn can_close_true_falls_through_to_guard() {
        // A permissive sugar signal does not auto-close: the explicit
        // guard still gets the final say (here it vetoes).
        let guard: CloseGuard = Rc::new(|_ctx| CloseResponse::Veto);
        let should_close = verdict(Some(Prop::from(Signal::new(true))), None, Some(guard));
        assert!(!should_close, "can_close == true still consults the guard");
    }

    #[test]
    fn can_close_true_without_guard_closes() {
        // on_close_blocked is set but must NOT fire when the signal is true.
        let fired = Rc::new(Cell::new(false));
        let flag = fired.clone();
        let on_blocked: CloseBlockedCallback = Rc::new(move |_ctx, _decision| flag.set(true));

        let should_close = verdict(Some(Prop::from(Signal::new(true))), Some(on_blocked), None);

        assert!(should_close, "a permissive signal with no guard closes");
        assert!(!fired.get(), "on_close_blocked must not fire when allowed");
    }

    /// A modal dismissal the application tree asks for (its own dialog
    /// window's answer runs elsewhere, so this is only ever a stray
    /// `dismiss_modal`) closes nothing, and is drained like a window's so
    /// the turn does not owe another pass for it.
    #[test]
    fn the_application_trees_modal_dismissal_is_drained() {
        let mut wm = WindowManager::new(teksilo_core::presets::intui::light());
        wm.install_app_tree(|tree| tree.add(crate::app::AppTreeAnchor));
        wm.app_tree_mut()
            .expect("installed")
            .run_with_event_context(&mut NoopWindowOps, |ctx| ctx.dismiss_modal());
        assert!(
            wm.app_tree()
                .expect("installed")
                .has_pending_modal_dismissal(),
            "the request is recorded"
        );
        assert!(wm.owes_turn_work());
        assert!(
            wm.drain_pending_modal_dismissals().is_empty(),
            "no window to close"
        );
        assert!(
            !wm.app_tree()
                .expect("installed")
                .has_pending_modal_dismissal()
        );
        assert!(!wm.owes_turn_work(), "drained, the turn owes nothing");
    }

    #[test]
    fn queue_close_is_forced_request_close_is_guarded() {
        let mut wm = WindowManager::new(teksilo_core::presets::intui::light());
        let a = TeksiloWindowId::new(1);
        let b = TeksiloWindowId::new(2);
        wm.queue_close(a);
        wm.request_close(b);

        assert_eq!(wm.pending_closes.len(), 2);
        let forced = wm.pending_closes.iter().find(|p| p.id == a).unwrap();
        let guarded = wm.pending_closes.iter().find(|p| p.id == b).unwrap();
        assert!(forced.force, "queue_close must enqueue a forced close");
        assert!(!guarded.force, "request_close must enqueue a guarded close");
    }

    /// The routes post the framework's private `WindowWake`: the state route
    /// always, the off-main draw route only where asked for (macOS), so on
    /// every other platform a draw wake posts no app event (spec AC2/AC18).
    #[test]
    fn wake_routes_post_draw_wakes_only_off_main() {
        use teksilo_canvas::wake::WakeKind;
        let posted: std::sync::Arc<std::sync::Mutex<Vec<crate::app::WindowWake>>> =
            Default::default();
        let post = {
            let posted = posted.clone();
            move |payload: Box<dyn std::any::Any + Send>| {
                let wake = payload
                    .downcast::<crate::app::WindowWake>()
                    .expect("a WindowWake payload");
                posted.lock().unwrap().push(*wake);
            }
        };
        let window = winit::window::WindowId::from(7u64);

        let (state, draw) = wake_routes(post.clone(), window, false);
        assert!(draw.is_none(), "no off-main draw route unless asked for");
        state();
        let got = posted.lock().unwrap().clone();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].window, window);
        assert_eq!(got[0].kind, WakeKind::Layout);

        let (_, draw) = wake_routes(post, window, true);
        draw.expect("the off-main draw route")();
        assert_eq!(posted.lock().unwrap()[1].kind, WakeKind::Draw);
    }

    /// Guard rail matching `close_window_on_an_unknown_id_is_a_harmless_no_op`
    /// below: with no windows open, `request_redraw_needing_render` must
    /// not panic and must report that it poked nothing. The substantive
    /// claim this method rests on — that reconciling a tree's reactive
    /// state before checking `needs_render()` is what actually surfaces a
    /// cross-window `Signal` mutation — is covered at the `WidgetTree`
    /// level in `teksilo_core::widget_tree::cross_window_redraw_signal_tests`,
    /// since a real `ManagedWindow` needs a real `PlatformWindow` this
    /// crate's headless tests cannot stand up.
    #[test]
    fn request_redraw_needing_render_on_an_empty_manager_pokes_nothing() {
        let mut wm = WindowManager::new(teksilo_core::presets::intui::light());
        assert_eq!(wm.request_redraw_needing_render(), 0);
    }

    /// `close_window` — the single choke point both `queue_close` (forced)
    /// and `request_close` (once guarded) funnel through, and the one
    /// place that fires `WindowConfig::on_removed` — must be a no-op for
    /// a `TeksiloWindowId` it has never seen (already closed, or never
    /// existed). Constructing a real `ManagedWindow` needs an actual
    /// `PlatformWindow` (a live winit window + wgpu surface), which is
    /// not available in a headless unit test — see `on_removed`'s own
    /// round-trip test in `teksilo_core::window::config` for the part of
    /// this feature that IS testable in isolation. This test instead
    /// pins down the guard-rail: no window, no hook to misfire, no panic.
    #[test]
    fn close_window_on_an_unknown_id_is_a_harmless_no_op() {
        let mut wm = WindowManager::new(teksilo_core::presets::intui::light());
        assert_eq!(wm.window_count(), 0);

        wm.close_window(TeksiloWindowId::new(42));

        assert_eq!(wm.window_count(), 0, "still no windows — nothing to remove");
    }
}

#[cfg(test)]
mod ime_dedup_tests {
    use super::ime_should_skip_empty_preedit;

    #[test]
    fn consecutive_empty_preedits_are_skipped_after_the_first() {
        let mut last_empty = false;

        // A real composition: non-empty preedits never skip and keep the flag low.
        assert!(!ime_should_skip_empty_preedit(&mut last_empty, false));
        assert!(!ime_should_skip_empty_preedit(&mut last_empty, false));
        assert!(!last_empty);

        // First empty preedit (winit's synthetic clear before commit, or the
        // start of a flood) is meaningful — dispatched, not skipped.
        assert!(!ime_should_skip_empty_preedit(&mut last_empty, true));
        assert!(last_empty);

        // Every consecutive empty preedit after it is a redundant no-op — skipped.
        assert!(ime_should_skip_empty_preedit(&mut last_empty, true));
        assert!(ime_should_skip_empty_preedit(&mut last_empty, true));

        // A non-empty preedit (or any non-empty IME event) resets the run, so the
        // NEXT empty preedit is again treated as meaningful.
        assert!(!ime_should_skip_empty_preedit(&mut last_empty, false));
        assert!(!last_empty);
        assert!(!ime_should_skip_empty_preedit(&mut last_empty, true));
        assert!(ime_should_skip_empty_preedit(&mut last_empty, true));
    }
}

#[cfg(test)]
mod window_close_subscription_purge_tests {
    use std::any::Any;
    use std::rc::Rc;
    use std::sync::Arc;

    use teksilo_canvas::SizeProposal;
    use teksilo_core::build_context::BuildContext;
    use teksilo_core::event_source::{
        AppEventPoster, EventSource, EventSourceAdapter, SubscriptionHandle, SubscriptionId,
        TreeAppContext,
    };
    use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};
    use teksilo_core::widget_id::WidgetId;
    use teksilo_core::{
        TeksiloWindowId, WidgetTree, WindowPlacement, WindowState, WindowStateInit,
    };

    use super::WindowManager;

    /// Accepts subscriptions and publishes nothing: this test is about the bookkeeping,
    /// not about delivery, so it never needs a token that unsubscribes.
    struct SilentSource;

    impl EventSource for SilentSource {
        type Origin = u32;
        type Event = u32;

        fn subscribe(
            &self,
            _origin: Self::Origin,
            _callback: Arc<dyn Fn(Self::Event) + Send + Sync + 'static>,
        ) -> SubscriptionHandle {
            SubscriptionHandle::empty()
        }
    }

    /// `subscribe_event` panics without a poster installed, and nothing here posts.
    struct SilentPoster;

    impl AppEventPoster for SilentPoster {
        fn post_subscription_event(&self, _sub_id: SubscriptionId, _event: Box<dyn Any + Send>) {}
    }

    /// Subscribes once in `build()`, the way every real widget does.
    #[derive(Debug)]
    struct Subscriber;

    impl Widget for Subscriber {
        fn build(&mut self, ctx: &mut BuildContext) -> Vec<WidgetId> {
            ctx.subscribe_event(0u32, |_event: &u32| {});
            Vec::new()
        }

        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }
    }

    fn window_state(id: u64) -> WindowState {
        WindowState::new(WindowStateInit {
            id: TeksiloWindowId::new(id),
            string_id: None,
            placement: WindowPlacement::Floating,
            title: String::new(),
            size: (800, 600),
            position: (0, 0),
            focused: true,
            resizable: true,
            always_on_top: false,
        })
    }

    /// `close_window` must purge the plain subscription map, not only the
    /// context-bearing one. The purge itself is unit-tested in
    /// `teksilo_core::event_source`; what this pins is the call site, which is the half
    /// that silently does not happen and that no core test can see.
    ///
    /// A real `ManagedWindow` needs a live `PlatformWindow`, which a headless test
    /// cannot stand up. It does not need one: the purge block sits above the `windows`
    /// lookup in `close_window`, so it runs for an id the manager never saw (the same
    /// property `close_window_on_an_unknown_id_is_a_harmless_no_op` relies on).
    #[test]
    fn closing_a_window_purges_the_plain_subscription_callbacks_it_installed() {
        let poster: Arc<dyn AppEventPoster> = Arc::new(SilentPoster);
        let template = Rc::new(TreeAppContext::with_source_and_poster(
            EventSourceAdapter::new(SilentSource),
            poster,
        ));

        let mut tree_one = WidgetTree::new();
        tree_one.set_window_state(window_state(1));
        tree_one.set_app_context(template.clone());
        tree_one.add(Subscriber);

        let mut tree_two = WidgetTree::new();
        tree_two.set_window_state(window_state(2));
        tree_two.set_app_context(template.clone());
        tree_two.add(Subscriber);

        assert_eq!(template.subscription_count(), 2);

        let mut wm = WindowManager::new(teksilo_core::presets::intui::light());
        wm.set_app_context_template(template.clone());

        wm.close_window(TeksiloWindowId::new(1));
        assert_eq!(
            template.subscription_count(),
            1,
            "close_window must drop window 1's plain callback and keep window 2's"
        );

        wm.close_window(TeksiloWindowId::new(2));
        assert_eq!(
            template.subscription_count(),
            0,
            "and the second window's on its own close"
        );
    }
}

#[cfg(test)]
mod window_menu_tests {
    use super::*;

    /// The macOS Window and Dock menus list the windows a user switches
    /// between: not a modal dialog, which comes forward with the window it
    /// blocks, nor a window kept out of the taskbar (a palette), which AppKit
    /// does not list as a panel either.
    #[test]
    fn dialogs_and_windows_kept_out_of_the_taskbar_are_not_listed() {
        assert!(listed_in_window_menu(&WindowConfig::new()));
        assert!(!listed_in_window_menu(
            &WindowConfig::new().modal_to(teksilo_core::window::TeksiloWindowId::new(1))
        ));
        assert!(!listed_in_window_menu(
            &WindowConfig::new().skip_taskbar(true)
        ));
    }
}

#[cfg(test)]
mod announcement_recording_tests {
    use super::*;

    /// The announcement ring's replay costs every sync a second pass over the
    /// tree, and in a running application only the automation bridge reads
    /// it: a window's tree records nothing unless the manager was asked.
    #[test]
    fn a_window_records_announcements_only_when_asked() {
        let theme = teksilo_core::presets::intui::light();
        let mut wm = WindowManager::new(theme.clone());
        assert!(!wm.new_window_tree(theme.clone()).records_announcements());
        wm.set_records_announcements(true);
        assert!(wm.new_window_tree(theme).records_announcements());
    }
}

#[cfg(test)]
mod os_theme_tests {
    use super::*;
    use crate::native_theme::NativeThemeRequest;
    use std::cell::Cell;
    use teksilo_core::presets::intui;
    use teksilo_platform::os_theme::NativeDesktop;
    use teksilo_tokens::ColorSchemePreference;
    use teksilo_tokens::{Color, OsThemeColors, TargetDensity};

    const DENSITIES: [TargetDensity; 2] = [TargetDensity::Compact, TargetDensity::Touch];

    fn accented(scheme: ColorSchemePreference) -> NativeDesktop {
        let mut os = OsThemeColors::new(scheme);
        os.accent = Some(Color::from_rgb(0.9, 0.2, 0.1));
        os.window_bg = Some(Color::from_rgb(0.94, 0.94, 0.95));
        NativeDesktop::new(os)
    }
    fn light_desktop() -> NativeDesktop {
        accented(ColorSchemePreference::Light)
    }
    fn dark_desktop() -> NativeDesktop {
        accented(ColorSchemePreference::Dark)
    }
    fn silent_desktop() -> NativeDesktop {
        NativeDesktop::new(OsThemeColors::new(ColorSchemePreference::NoPreference))
    }
    fn says_light() -> ColorSchemePreference {
        ColorSchemePreference::Light
    }
    fn says_dark() -> ColorSchemePreference {
        ColorSchemePreference::Dark
    }
    fn says_nothing() -> ColorSchemePreference {
        ColorSchemePreference::NoPreference
    }

    fn queries(
        native_desktop: fn() -> NativeDesktop,
        color_scheme: fn() -> ColorSchemePreference,
    ) -> OsQueries {
        OsQueries {
            native_desktop,
            color_scheme,
            accessibility: AccessibilityPreferences::default,
        }
    }

    /// Every pair of readings: each desktop with each light or dark answer.
    fn all_queries() -> Vec<OsQueries> {
        let mut out = Vec::new();
        for desktop in [light_desktop, dark_desktop, silent_desktop] {
            for scheme in [says_light, says_dark, says_nothing] {
                out.push(queries(desktop, scheme));
            }
        }
        out
    }

    /// A manager in `mode` whose theme is at `density`, holding the
    /// accessibility preferences [`queries`] reads.
    fn manager(mode: ThemeMode, density: TargetDensity) -> WindowManager {
        let mut wm = WindowManager::new(intui::light().with_density(density));
        wm.a11y_prefs = AccessibilityPreferences::default();
        wm.set_theme_mode(mode);
        wm
    }

    /// The theme `source` resolves to at the presets' density, projected
    /// onto `density`.
    fn projected(source: OsThemeSource, density: TargetDensity) -> Theme {
        resolve_os_theme(None, &source.at(TargetDensity::Compact)).with_density(density)
    }

    #[test]
    fn a_window_opened_keeps_the_density() {
        for density in DENSITIES {
            for hint in [None, Some(false), Some(true)] {
                let mut wm = manager(ThemeMode::Native, density);
                let (theme, inputs) =
                    wm.new_window_theme(hint, queries(dark_desktop, says_nothing));
                assert_eq!(theme.input.density, density);
                assert_eq!(
                    theme,
                    projected(OsThemeSource::native(dark_desktop(), hint), density)
                );
                assert_eq!(wm.theme, theme);
                assert_eq!(inputs.as_ref().map(|i| i.density), Some(density));

                let mut wm = manager(ThemeMode::FollowSystem, density);
                let (theme, _) = wm.new_window_theme(hint, queries(dark_desktop, says_dark));
                assert_eq!(theme.input.density, density);
                assert_eq!(
                    theme,
                    projected(OsThemeSource::follow_system(true), density)
                );
            }
        }
    }

    #[test]
    fn an_os_theme_change_keeps_the_density() {
        for density in DENSITIES {
            let mut wm = manager(ThemeMode::Native, density);
            wm.apply_os_theme_from(Some(true), queries(light_desktop, says_light));
            assert_eq!(wm.theme.input.density, density);
            assert_eq!(
                wm.theme,
                projected(OsThemeSource::native(light_desktop(), Some(true)), density)
            );

            let mut wm = manager(ThemeMode::FollowSystem, density);
            wm.apply_os_theme_from(None, queries(light_desktop, says_dark));
            assert_eq!(wm.theme.input.density, density);
            assert_eq!(
                wm.theme,
                projected(OsThemeSource::follow_system(true), density)
            );
        }
    }

    #[test]
    fn following_the_system_theme_keeps_the_density() {
        for density in DENSITIES {
            let mut wm = manager(ThemeMode::Manual, density);
            wm.follow_system_theme(queries(dark_desktop, says_light));
            assert_eq!(wm.theme_mode(), ThemeMode::Native);
            assert_eq!(wm.theme.input.density, density);
            assert_eq!(
                wm.theme,
                projected(OsThemeSource::native(dark_desktop(), None), density)
            );
            assert!(wm.theme.is_dark());
        }
    }

    #[test]
    fn a_manual_theme_ignores_the_os() {
        let theme = intui::dark().with_density(TargetDensity::Touch);
        let mut wm = WindowManager::new(theme.clone());
        wm.apply_os_theme_from(None, queries(light_desktop, says_light));
        assert_eq!(wm.theme, theme);
        assert_eq!(
            wm.new_window_theme(Some(false), queries(light_desktop, says_light)),
            (theme, None)
        );
    }

    #[test]
    fn a_window_opened_on_a_desktop_with_no_preference_follows_its_hint() {
        for density in DENSITIES {
            for (hint, dark) in [(Some(true), true), (Some(false), false), (None, false)] {
                let mut wm = manager(ThemeMode::Native, density);
                let (theme, _) = wm.new_window_theme(hint, queries(silent_desktop, says_nothing));
                assert_eq!(theme.is_dark(), dark, "hint {hint:?}");
                let preset = if dark { intui::dark() } else { intui::light() };
                assert_eq!(theme, preset.with_density(density).with_id("system"));
            }
        }
    }

    #[test]
    fn a_window_opened_under_follow_system_reads_the_desktops_preference() {
        // On Linux winit gives a window no light or dark hint, so the
        // desktop's own answer is all there is to open on.
        for density in DENSITIES {
            for (scheme, dark) in [
                (says_dark as fn() -> ColorSchemePreference, true),
                (says_light, false),
            ] {
                for hint in [None, Some(false), Some(true)] {
                    let mut wm = manager(ThemeMode::FollowSystem, density);
                    let (theme, inputs) =
                        wm.new_window_theme(hint, queries(silent_desktop, scheme));
                    assert_eq!(theme.is_dark(), dark, "hint {hint:?}");
                    assert_eq!(
                        inputs,
                        Some(OsThemeSource::follow_system(dark).at(density)),
                        "hint {hint:?}"
                    );
                    assert_eq!(wm.theme, theme, "later windows start from it");
                }
            }
            for (hint, dark) in [(None, false), (Some(false), false), (Some(true), true)] {
                let mut wm = manager(ThemeMode::FollowSystem, density);
                let (theme, _) = wm.new_window_theme(hint, queries(silent_desktop, says_nothing));
                assert_eq!(theme.is_dark(), dark, "no preference follows hint {hint:?}");
            }
        }
    }

    #[test]
    fn a_second_window_keeps_the_dark_a_live_change_set() {
        let mut wm = manager(ThemeMode::FollowSystem, TargetDensity::Compact);
        let dark = queries(light_desktop, says_dark);
        wm.new_window_theme(None, dark);
        wm.apply_os_theme_from(None, dark);
        let (second, _) = wm.new_window_theme(None, dark);
        assert!(second.is_dark());
        assert!(wm.theme.is_dark());
    }

    #[test]
    fn a_window_opened_and_a_live_change_agree() {
        for mode in [ThemeMode::FollowSystem, ThemeMode::Native] {
            for q in all_queries() {
                for hint in [None, Some(false), Some(true)] {
                    for density in DENSITIES {
                        let mut opened = manager(mode, density);
                        let (theme, inputs) = opened.new_window_theme(hint, q);
                        let mut live = manager(mode, density);
                        live.apply_os_theme_from(hint, q);
                        assert_eq!(theme, live.theme, "{mode:?}, hint {hint:?}, {density:?}");
                        assert_eq!(inputs, live.os_theme);
                    }
                }
            }
        }
    }

    #[test]
    fn an_unchanged_reading_resolves_and_sets_nothing() {
        let calls = Rc::new(Cell::new(0));
        let count = calls.clone();
        let mut wm = manager(ThemeMode::Native, TargetDensity::Touch);
        wm.set_native_theme(Some(Box::new(move |_: &NativeThemeRequest<'_>| {
            count.set(count.get() + 1);
            None
        })));

        // A window opens, then the OS reports what it was built from.
        let (opened, _) = wm.new_window_theme(None, queries(silent_desktop, says_nothing));
        assert_eq!(calls.get(), 1);
        wm.apply_os_theme_from(None, queries(silent_desktop, says_nothing));
        wm.apply_os_theme_from(Some(false), queries(silent_desktop, says_nothing));
        assert_eq!(calls.get(), 1, "the same desktop, state and density");
        assert_eq!(wm.theme, opened);

        // A changed light or dark state.
        wm.apply_os_theme_from(Some(true), queries(silent_desktop, says_nothing));
        assert_eq!(calls.get(), 2);
        assert!(wm.theme.is_dark());
        wm.apply_os_theme_from(Some(true), queries(silent_desktop, says_nothing));
        assert_eq!(calls.get(), 2);

        // A changed desktop.
        wm.apply_os_theme_from(Some(true), queries(dark_desktop, says_nothing));
        assert_eq!(calls.get(), 3);
        wm.apply_os_theme_from(None, queries(dark_desktop, says_nothing));
        assert_eq!(calls.get(), 3, "a stated preference ignores the hint");

        // A changed density: a theme the app set at another density.
        wm.set_theme(intui::light().with_density(TargetDensity::Comfortable));
        wm.apply_os_theme_from(None, queries(dark_desktop, says_nothing));
        assert_eq!(calls.get(), 4);
        assert_eq!(wm.theme.input.density, TargetDensity::Comfortable);

        // Any theme set in between is replaced, even at the same density.
        wm.set_theme(intui::light().with_density(TargetDensity::Comfortable));
        wm.apply_os_theme_from(None, queries(dark_desktop, says_nothing));
        assert_eq!(calls.get(), 5);
        assert!(wm.theme.is_dark());
    }
    /// The light desktop with another accent.
    fn reaccented_desktop() -> NativeDesktop {
        let mut desktop = light_desktop();
        desktop.os.accent = Some(Color::from_rgb(0.1, 0.5, 0.3));
        desktop
    }

    /// A change the watch posts once.
    fn changed() -> Box<dyn std::any::Any + Send> {
        Box::new(teksilo_platform::os_theme::OsThemeChanged)
    }

    #[test]
    fn a_desktop_change_rereads_the_desktop_and_applies_what_changed() {
        let mut wm = manager(ThemeMode::Native, TargetDensity::Touch);
        let route = |wm: &mut WindowManager, desktop: fn() -> NativeDesktop| {
            wm.route_os_theme_payload_from(changed(), queries(desktop, says_nothing))
                .expect("the watch's payload is taken")
        };
        assert!(route(&mut wm, light_desktop), "the first reading sets");
        assert_eq!(
            wm.theme,
            projected(
                OsThemeSource::native(light_desktop(), None),
                TargetDensity::Touch
            )
        );
        assert!(
            !route(&mut wm, light_desktop),
            "an unchanged desktop sets nothing"
        );

        let before = wm.theme.clone();
        assert!(route(&mut wm, reaccented_desktop), "a new accent");
        assert_eq!(
            wm.theme,
            projected(
                OsThemeSource::native(reaccented_desktop(), None),
                TargetDensity::Touch
            )
        );
        assert_ne!(wm.theme.colors, before.colors);
        assert!(!wm.theme.is_dark());

        assert!(route(&mut wm, dark_desktop), "a dark scheme");
        assert!(wm.theme.is_dark());
        assert_eq!(wm.theme.input.density, TargetDensity::Touch);
    }

    #[test]
    fn a_desktop_change_follows_the_light_or_dark_preference() {
        let mut wm = manager(ThemeMode::FollowSystem, TargetDensity::Compact);
        let route = |wm: &mut WindowManager, scheme: fn() -> ColorSchemePreference| {
            wm.route_os_theme_payload_from(changed(), queries(light_desktop, scheme))
                .expect("the watch's payload is taken")
        };
        assert!(route(&mut wm, says_light));
        assert!(!wm.theme.is_dark());
        assert!(route(&mut wm, says_dark));
        assert!(wm.theme.is_dark());
        assert!(
            !route(&mut wm, says_dark),
            "the same preference sets nothing"
        );
    }

    #[test]
    fn a_desktop_change_leaves_a_manual_theme_alone() {
        let theme = intui::dark().with_density(TargetDensity::Touch);
        let mut wm = WindowManager::new(theme.clone());
        wm.a11y_prefs = AccessibilityPreferences::default();
        assert_eq!(
            wm.route_os_theme_payload_from(changed(), queries(light_desktop, says_light))
                .ok(),
            Some(false)
        );
        assert_eq!(wm.theme, theme);
        assert!(!wm.recheck_os_theme_files(), "no watch, nothing to check");
    }

    #[test]
    fn any_other_payload_is_handed_back_untouched() {
        let mut wm = manager(ThemeMode::Native, TargetDensity::Compact);
        let theme = wm.theme.clone();
        let back = wm
            .route_os_theme_payload_from(Box::new(7_u8), queries(dark_desktop, says_dark))
            .expect_err("not the watch's payload");
        assert_eq!(back.downcast_ref::<u8>(), Some(&7));
        assert_eq!(wm.theme, theme, "and the desktop was not read");
    }

    thread_local! {
        /// How many [`FakeWatch`]es this test thread started, and how many
        /// are alive.
        static WATCHES_STARTED: Cell<usize> = const { Cell::new(0) };
        static WATCHES_ALIVE: Cell<usize> = const { Cell::new(0) };
    }

    /// A watch that hears nothing and counts itself.
    struct FakeWatch;

    impl DesktopWatch for FakeWatch {
        fn files_changed(&mut self) -> bool {
            false
        }
    }

    impl Drop for FakeWatch {
        fn drop(&mut self) {
            WATCHES_ALIVE.with(|alive| alive.set(alive.get() - 1));
        }
    }

    /// Starts a [`FakeWatch`], with or without an event proxy.
    fn start_fake_watch(_proxy: Option<&AppEventProxy>) -> Option<Box<dyn DesktopWatch>> {
        WATCHES_STARTED.with(|started| started.set(started.get() + 1));
        WATCHES_ALIVE.with(|alive| alive.set(alive.get() + 1));
        Some(Box::new(FakeWatch))
    }

    /// A manager in `mode` that starts [`FakeWatch`]es.
    fn watching_manager(mode: ThemeMode) -> WindowManager {
        WATCHES_STARTED.with(|started| started.set(0));
        WATCHES_ALIVE.with(|alive| alive.set(0));
        let mut wm = manager(ThemeMode::Manual, TargetDensity::Compact);
        wm.start_os_theme_watch = start_fake_watch;
        wm.set_theme_mode(mode);
        wm
    }

    fn watches() -> (usize, usize) {
        (
            WATCHES_STARTED.with(Cell::get),
            WATCHES_ALIVE.with(Cell::get),
        )
    }

    #[test]
    fn the_platform_watch_waits_for_the_event_proxy() {
        assert!(start_platform_desktop_watch(None).is_none());
        let mut wm = manager(ThemeMode::Native, TargetDensity::Compact);
        wm.sync_os_theme_watch();
        assert!(wm.os_theme_watch.is_none(), "no proxy, no watch");
    }

    #[test]
    fn the_watch_runs_while_the_theme_follows_the_os_and_stops_under_manual() {
        for mode in [ThemeMode::Native, ThemeMode::FollowSystem] {
            let mut wm = watching_manager(mode);
            assert!(wm.os_theme_watch.is_some(), "{mode:?}");
            assert_eq!(watches(), (1, 1), "{mode:?}");
            wm.sync_os_theme_watch();
            wm.set_theme_mode(ThemeMode::Native);
            wm.set_theme_mode(ThemeMode::FollowSystem);
            assert_eq!(watches(), (1, 1), "{mode:?}: one watch, kept");
            wm.set_theme_mode(ThemeMode::Manual);
            assert!(wm.os_theme_watch.is_none(), "{mode:?}");
            assert_eq!(watches(), (1, 0), "{mode:?}: stopped");
        }
        let wm = watching_manager(ThemeMode::Manual);
        assert!(wm.os_theme_watch.is_none());
        assert_eq!(watches(), (0, 0), "Manual starts none");
    }

    #[test]
    fn an_explicit_theme_pick_stops_the_watch() {
        let mut wm = watching_manager(ThemeMode::Native);
        assert!(!wm.apply_theme_requests(Vec::new()), "no request");
        assert_eq!(wm.theme_mode(), ThemeMode::Native);
        assert_eq!(watches(), (1, 1), "no request, the watch runs on");

        assert!(wm.apply_theme_requests(vec![intui::dark()]));
        assert_eq!(wm.theme_mode(), ThemeMode::Manual);
        assert!(wm.os_theme_watch.is_none());
        assert_eq!(watches(), (1, 0), "a pick stops it");
        assert!(wm.theme.is_dark());
    }

    #[test]
    fn following_the_system_starts_the_watch() {
        let mut wm = watching_manager(ThemeMode::Manual);
        wm.follow_system_theme(queries(light_desktop, says_light));
        assert!(wm.os_theme_watch.is_some());
        assert_eq!(watches(), (1, 1));
    }

    /// The defaults with reduced motion asked for, as KDE's animation speed
    /// set to instant reads.
    fn reduced_motion() -> AccessibilityPreferences {
        AccessibilityPreferences {
            reduced_motion: true,
            ..AccessibilityPreferences::default()
        }
    }

    #[test]
    fn a_desktop_change_reads_the_accessibility_preferences_again() {
        let mut wm = manager(ThemeMode::Native, TargetDensity::Compact);
        let q = queries(light_desktop, says_nothing);
        assert!(wm.route_os_theme_payload_from(changed(), q).expect("taken"));
        assert!(
            !wm.route_os_theme_payload_from(changed(), q).expect("taken"),
            "nothing changed"
        );

        let slowed = OsQueries {
            accessibility: reduced_motion,
            ..q
        };
        assert!(
            wm.route_os_theme_payload_from(changed(), slowed)
                .expect("taken"),
            "the preferences alone changed"
        );
        assert!(wm.a11y_prefs.reduced_motion);
        assert!(
            !wm.route_os_theme_payload_from(changed(), slowed)
                .expect("taken")
        );
    }
}

#[cfg(test)]
mod app_tree_tests {
    use teksilo_canvas::SizeProposal;
    use teksilo_core::widget::{LayoutContext, LayoutResponse, Widget};

    use super::*;

    /// A zero-size root.
    #[derive(Debug)]
    struct Anchor;

    impl Widget for Anchor {
        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }
    }

    fn manager_with_app_tree() -> WindowManager {
        let mut wm = WindowManager::new(teksilo_core::presets::intui::light());
        wm.install_app_tree(|tree| tree.add(Anchor));
        wm
    }

    #[test]
    fn the_app_tree_has_its_root_and_no_window() {
        let mut wm = WindowManager::new(teksilo_core::presets::intui::light());
        wm.set_records_announcements(true);
        let root = wm.install_app_tree(|tree| tree.add(Anchor));
        let tree = wm.app_tree().expect("installed");
        assert_eq!(tree.roots(), [root]);
        assert!(tree.window_state().is_none(), "no window hosts it");
        assert!(
            !tree.records_announcements(),
            "nothing reads announcements from a tree no adapter serves"
        );
        assert_eq!(wm.window_count(), 0, "it is not a window");
    }

    #[test]
    fn the_app_tree_starts_with_the_applications_text_scale() {
        let mut wm = WindowManager::new(teksilo_core::presets::intui::light());
        wm.set_initial_text_scale(1.5);
        wm.install_app_tree(|tree| tree.add(Anchor));
        assert_eq!(wm.app_tree().unwrap().user_text_scale(), 1.5);
    }

    #[test]
    fn a_theme_change_reaches_the_app_tree() {
        let mut wm = manager_with_app_tree();
        let dark = teksilo_core::presets::intui::dark();
        wm.set_theme(dark.clone());
        assert_eq!(wm.app_tree().unwrap().theme().id, dark.id);
    }

    /// A theme change made while the tree is taken out (a window opened
    /// from one of its handlers resolving the OS theme) reaches it when it
    /// is put back.
    #[test]
    fn a_theme_change_made_while_the_app_tree_is_out_reaches_it_on_return() {
        let mut wm = manager_with_app_tree();
        let tree = wm.take_app_tree().expect("installed");
        let dark = teksilo_core::presets::intui::dark();
        wm.set_theme(dark.clone());
        wm.reinsert_app_tree(tree);
        assert_eq!(wm.app_tree().unwrap().theme().id, dark.id);
    }

    #[test]
    fn a_text_scale_change_reaches_the_app_tree() {
        let mut wm = manager_with_app_tree();
        wm.set_text_scale(1.25);
        assert_eq!(wm.app_tree().unwrap().user_text_scale(), 1.25);
    }

    #[test]
    fn an_accessibility_preference_change_reaches_the_app_tree() {
        let mut wm = manager_with_app_tree();
        fn high_contrast() -> AccessibilityPreferences {
            AccessibilityPreferences {
                high_contrast: true,
                ..AccessibilityPreferences::default()
            }
        }
        assert!(wm.refresh_accessibility_preferences_from(high_contrast));
        assert!(wm.app_tree().unwrap().high_contrast_signal().get());
    }

    #[test]
    fn a_locale_change_reaches_the_app_tree() {
        teksilo_i18n::thread_local::install(teksilo_i18n::I18nManager::from_config(
            &teksilo_i18n::I18nConfig::test_only("en-US", &[]),
        ));
        let mut wm = manager_with_app_tree();
        assert_eq!(wm.app_tree().unwrap().locale(), Some("en-US"), "seeded");
        wm.set_locale("fr-FR".parse().unwrap());
        assert_eq!(wm.app_tree().unwrap().locale(), Some("fr-FR"));
        teksilo_i18n::thread_local::clear();
    }
}

#[cfg(test)]
mod app_tree_drain_tests {
    use teksilo_canvas::SizeProposal;
    use teksilo_core::NoopWindowOps;
    use teksilo_core::widget::{EventContext, LayoutContext, LayoutResponse, Widget};

    use super::*;

    #[derive(Debug)]
    struct Anchor;

    impl Widget for Anchor {
        fn layout_response(&self, proposal: SizeProposal, _ctx: &LayoutContext) -> LayoutResponse {
            proposal.resolve(0.0, 0.0).into()
        }
    }

    /// A manager whose application tree ran `handler`, as one of its
    /// handlers would.
    fn after(handler: impl FnOnce(&mut EventContext)) -> WindowManager {
        let mut wm = WindowManager::new(teksilo_core::presets::intui::light());
        wm.install_app_tree(|tree| tree.add(Anchor));
        wm.app_tree_mut()
            .unwrap()
            .run_with_event_context(&mut NoopWindowOps, handler);
        wm
    }

    #[test]
    fn a_text_scale_asked_by_the_app_tree_is_applied_everywhere() {
        let mut wm = after(|ctx| ctx.set_text_scale(1.3));
        assert!(wm.drain_pending_text_scale_requests());
        assert_eq!(wm.app_tree().unwrap().user_text_scale(), 1.3);
        assert!(!wm.drain_pending_text_scale_requests(), "drained once");
    }

    #[test]
    fn a_theme_asked_by_the_app_tree_is_applied_everywhere() {
        let dark = teksilo_core::presets::intui::dark();
        let requested = dark.clone();
        let mut wm = after(move |ctx| ctx.set_theme(requested));
        assert!(wm.drain_pending_theme_requests());
        assert_eq!(wm.theme().id, dark.id);
        assert_eq!(wm.app_tree().unwrap().theme().id, dark.id);
    }

    #[test]
    fn following_the_system_asked_by_the_app_tree_switches_the_mode() {
        let mut wm = after(|ctx| ctx.follow_system_theme());
        assert!(wm.drain_pending_follow_system_requests());
        assert_eq!(wm.theme_mode(), ThemeMode::Native);
    }

    #[test]
    fn a_locale_asked_by_the_app_tree_is_applied_everywhere() {
        teksilo_i18n::thread_local::install(teksilo_i18n::I18nManager::from_config(
            &teksilo_i18n::I18nConfig::test_only("en-US", &[]),
        ));
        let mut wm = after(|ctx| ctx.set_locale("fr-FR"));
        assert!(wm.drain_pending_locale_requests());
        assert_eq!(wm.app_tree().unwrap().locale(), Some("fr-FR"));
        teksilo_i18n::thread_local::clear();
    }

    /// The application tree is no window: its close request queues nothing,
    /// and does not stay to fire later.
    #[test]
    fn a_close_asked_by_the_app_tree_closes_nothing_and_is_drained() {
        let mut wm = after(|ctx| {
            ctx.close_window();
            ctx.close_window_forced();
        });
        assert!(!wm.drain_close_window_requests());
        assert!(wm.pending_closes.is_empty());
        let tree = wm.app_tree_mut().unwrap();
        assert!(!tree.take_close_window_request());
        assert!(!tree.take_force_close_request());
    }
}

#[cfg(test)]
mod activation_token_slot_tests {
    use super::*;

    #[test]
    fn the_token_is_taken_once_and_cleared_unused() {
        let mut wm = WindowManager::new(teksilo_core::presets::intui::light());
        assert_eq!(wm.take_dispatch_activation_token(), None);
        wm.set_dispatch_activation_token("token".into());
        assert_eq!(
            wm.take_dispatch_activation_token().as_deref(),
            Some("token")
        );
        assert_eq!(wm.take_dispatch_activation_token(), None, "taken once");
        wm.set_dispatch_activation_token("unused".into());
        wm.clear_dispatch_activation_token();
        assert_eq!(wm.take_dispatch_activation_token(), None);
    }

    /// A handler left its event's token for the focus it queued; an intent
    /// routed afterwards in the same turn, whose action used the token it
    /// carried, does not take that one away when it is released.
    #[test]
    fn a_routed_intents_release_keeps_the_token_left_for_a_queued_focus() {
        let mut wm = WindowManager::new(teksilo_core::presets::intui::light());
        wm.set_dispatch_activation_token("event".into());
        wm.end_dispatch_activation();
        // `WindowOps::hold_activation_token`, its action raising a window,
        // then `release_activation_token`.
        wm.set_dispatch_activation_token("routed".into());
        assert_eq!(
            wm.use_dispatch_activation_token().as_deref(),
            Some("routed")
        );
        wm.end_dispatch_activation();
        assert_eq!(wm.focus_command_token.as_deref(), Some("event"));
    }

    /// The same, with a routed intent whose action left its token unused:
    /// the token waiting for the queued focus is the one its handler left,
    /// not the one released after it.
    #[test]
    fn an_unused_routed_token_does_not_replace_the_token_left_for_a_queued_focus() {
        let mut wm = WindowManager::new(teksilo_core::presets::intui::light());
        wm.set_dispatch_activation_token("event".into());
        wm.end_dispatch_activation();
        wm.set_dispatch_activation_token("routed".into());
        wm.end_dispatch_activation();
        assert_eq!(wm.focus_command_token.as_deref(), Some("event"));
        assert!(!wm.holds_dispatch_activation_token());
    }
}
