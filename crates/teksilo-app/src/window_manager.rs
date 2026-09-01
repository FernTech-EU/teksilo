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
use teksilo_tokens::{ColorSchemePreference, ColorTokens};
#[allow(unused_imports)]
use winit::raw_window_handle::HasWindowHandle;
use winit::window::WindowLevel;

use crate::app::{AppEventProxy, CloseWindowRequest, ThemeMode};

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
/// [`WindowManager::evaluate_close_guard`]. Returns `true` if the close
/// should proceed, `false` to veto.
///
/// Precedence:
/// 1. [`can_close`](teksilo_core::WindowConfig::can_close) sugar — a
///    `Some(false)` signal vetoes and fires `on_close_blocked` (the
///    only side effect here).
/// 2. the [`on_close_requested`](teksilo_core::WindowConfig::on_close_requested)
///    guard's [`CloseResponse`].
/// 3. no guard configured → close.
///
/// Factored out of `evaluate_close_guard` so the decision can be
/// unit-tested headlessly with a `NoopWindowOps`-backed `EventContext`,
/// without standing up a winit event loop.
fn close_verdict(
    can_close: &Option<Prop<bool>>,
    on_close_blocked: &Option<CloseBlockedCallback>,
    close_guard: &Option<CloseGuard>,
    ctx: &mut teksilo_core::widget::EventContext,
) -> bool {
    if let Some(may_close) = can_close
        && !may_close.get()
    {
        if let Some(on_blocked) = on_close_blocked {
            on_blocked(ctx);
        }
        return false;
    }
    if let Some(guard) = close_guard {
        return matches!(guard(ctx), CloseResponse::Close);
    }
    true
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
    /// Last height (logical px) applied by the size-to-content auto-resize.
    /// Guards against a measure → resize → re-measure oscillation: a resize is
    /// only issued when the freshly-measured target differs from this.
    pub last_autosize_height: Option<u32>,
    /// Custom-chrome host, if the window opted in via
    /// `WindowConfig::custom_chrome(true)` and the platform supports it.
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
    /// Tracks `WindowEvent::Occluded`. macOS-only in winit 0.30 —
    /// stays `false` on every other platform. Combined with `focused`
    /// to decide whether the widget tree's animation scheduler should
    /// run: `active = focused && !occluded`.
    pub occluded: bool,
    /// Caps Lock active state, toggled on each `Key::CapsLock` press
    /// (winit 0.30 delivers Caps Lock as a discrete key, not via
    /// `ModifiersState`). Pushed to `state.caps_lock` so password fields
    /// can warn. Starts `false`; the OS lock state at launch is not
    /// observable on winit 0.30, so it can desync if Caps Lock was
    /// already on before the app gained focus.
    pub caps_lock_active: bool,
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
    /// When present and `false`, a guarded close is vetoed and
    /// [`on_close_blocked`](Self::on_close_blocked) fires. Evaluated
    /// before [`close_guard`](Self::close_guard).
    pub can_close: Option<Prop<bool>>,
    /// Optional notification fired when [`can_close`](Self::can_close)
    /// blocks a close, from
    /// [`WindowConfig::on_close_blocked`](teksilo_core::WindowConfig::on_close_blocked).
    pub on_close_blocked: Option<CloseBlockedCallback>,
    /// Optional teardown hook from
    /// [`WindowConfig::on_removed`](teksilo_core::WindowConfig::on_removed).
    /// Invoked once by [`close_window`](WindowManager::close_window),
    /// after this window is fully gone from every `WindowManager`
    /// registry — see that method's doc comment for why.
    pub on_removed: Option<teksilo_core::WindowRemovedCallback>,
    /// Glyph-atlas content version last uploaded to THIS window's
    /// renderer (`AtlasInfo::version`). Each window compares this
    /// against the shared bridge's current version on its own redraw
    /// and re-uploads when behind — the version model replaces
    /// consume-once dirty semantics so several windows all converge on
    /// the same atlas content. `0` = nothing uploaded yet; stays `0`
    /// (unused) when the `text` feature is off.
    pub atlas_uploaded_version: u64,
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
    #[cfg(feature = "text")]
    typesetter: Option<teksilo_text::SharedTypesetter>,
    /// Windows that are blocked by a modal child.
    modal_blocked: HashMap<TeksiloWindowId, TeksiloWindowId>,
    /// OS-level accessibility preferences, queried once at startup.
    a11y_prefs: AccessibilityPreferences,
    /// User-controlled global text-scale factor (`1.0` = 100 %). Seeded from
    /// `teksilo_settings::TEXT_SCALE_KEY` before the first window opens and
    /// applied to every tree created afterwards; updated at runtime via
    /// [`set_text_scale`](Self::set_text_scale).
    user_text_scale: f32,
    /// How the app resolves its theme (Manual, FollowSystem, Native).
    theme_mode: ThemeMode,
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
}

impl WindowManager {
    pub fn new(theme: Theme) -> Self {
        let a11y_prefs = AccessibilityPreferences::query();
        Self {
            windows: HashMap::new(),
            teksilo_to_winit: HashMap::new(),
            string_to_id: HashMap::new(),
            next_id: 1,
            pending_closes: Vec::new(),
            theme,
            #[cfg(feature = "text")]
            typesetter: None,
            modal_blocked: HashMap::new(),
            a11y_prefs,
            user_text_scale: 1.0,
            theme_mode: ThemeMode::Manual,
            app_context_template: None,
            event_proxy: None,
            pending_token_callbacks: HashMap::new(),
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

    /// Set the theme mode (called by TeksiloAppHandler during initialization).
    pub fn set_theme_mode(&mut self, mode: ThemeMode) {
        self.theme_mode = mode;
    }

    /// Install the event-loop proxy (called by TeksiloAppHandler once the
    /// proxy is available). Enables `TitleBarHostCallbacks::request_close`
    /// to post `CloseWindowRequest` back through the event loop.
    pub fn set_event_proxy(&mut self, proxy: AppEventProxy) {
        self.event_proxy = Some(proxy);
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
    /// adopts the OS's actual colours (GNOME/KDE/Cinnamon on Linux); under
    /// `FollowSystem` it picks the built-in light/dark preset. Both results
    /// carry the id `"system"`.
    ///
    /// `os_dark_hint` is the OS light/dark state as reported by winit (e.g.
    /// from a `WindowEvent::ThemeChanged`), used as the authoritative source on
    /// platforms where Teksilo's own OS-colour query is unimplemented
    /// (macOS / Windows, where `query_os_theme_colors()` returns
    /// `NoPreference`). On Linux the query reports a real scheme and the hint
    /// is unused. Pass `None` when no winit signal is available (e.g. a runtime
    /// "follow system" request) — the current window's reported theme is used
    /// where possible, otherwise it resolves to light.
    pub fn apply_os_theme(&mut self, os_dark_hint: Option<bool>) {
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
        let theme = match self.theme_mode {
            ThemeMode::Manual => return,
            ThemeMode::FollowSystem => {
                let dark = match teksilo_platform::os_theme::query_color_scheme() {
                    ColorSchemePreference::Dark => true,
                    ColorSchemePreference::Light => false,
                    ColorSchemePreference::NoPreference => hint.unwrap_or(false),
                };
                if dark {
                    teksilo_core::presets::intui::dark()
                } else {
                    teksilo_core::presets::intui::light()
                }
                .with_id("system")
            }
            ThemeMode::Native => {
                let os = teksilo_platform::os_theme::query_os_theme_colors();
                match os.color_scheme {
                    // Real OS scheme (Linux): adopt the OS's actual colours.
                    ColorSchemePreference::Dark => Theme {
                        colors: ColorTokens::from_os_colors(&os),
                        ..teksilo_core::presets::intui::dark()
                    },
                    ColorSchemePreference::Light => Theme {
                        colors: ColorTokens::from_os_colors(&os),
                        ..teksilo_core::presets::intui::light()
                    },
                    // No OS-colour support (macOS/Windows): follow the winit
                    // light/dark hint using the built-in presets.
                    ColorSchemePreference::NoPreference => {
                        if hint.unwrap_or(false) {
                            teksilo_core::presets::intui::dark()
                        } else {
                            teksilo_core::presets::intui::light()
                        }
                    }
                }
                .with_id("system")
            }
        };
        self.set_theme(theme);
    }

    #[cfg(feature = "text")]
    pub fn set_typesetter(&mut self, typesetter: teksilo_text::SharedTypesetter) {
        self.typesetter = Some(typesetter);
    }

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
        mut config: WindowConfig,
        target: &winit::event_loop::ActiveEventLoop,
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
        if let Some(sid) = &config.string_id {
            self.string_to_id.insert(sid.clone(), teksilo_id);
        }
        let wants_custom_chrome = config.decorations.wants_custom_chrome_host();
        let is_modal = config.is_modal();
        let modal_parent = config.modal_parent();
        let modal_focus_target = config.modal_focus_target();

        // Center modal windows over their parent when the caller did not
        // request a specific position. Approximates the modal's outer
        // rect with its inner (client) size — close enough visually
        // since decoration thickness is small relative to the dialog.
        // No-op on Wayland (compositor owns positioning).
        if config.position.is_none()
            && let Some(parent_id) = modal_parent
            && let Some(parent_winit) = self.winit_id_for_teksilo(parent_id)
            && let Some(parent_managed) = self.windows.get(&parent_winit)
        {
            let parent_window = parent_managed.platform_window.window();
            if let Ok(parent_outer_pos) = parent_window.outer_position() {
                let parent_sf = parent_window.scale_factor();
                let parent_outer_size = parent_window.outer_size();
                let p_x = parent_outer_pos.x as f64 / parent_sf;
                let p_y = parent_outer_pos.y as f64 / parent_sf;
                let p_w = parent_outer_size.width as f64 / parent_sf;
                let p_h = parent_outer_size.height as f64 / parent_sf;
                let m_w = config.size.0 as f64;
                let m_h = config.size.1 as f64;
                let x = (p_x + (p_w - m_w) / 2.0).round() as i32;
                let y = (p_y + (p_h - m_h) / 2.0).round() as i32;
                config.position = Some((x, y));
            }
        }

        let mut window_attrs = winit::window::Window::default_attributes()
            .with_title(&config.title)
            .with_inner_size(winit::dpi::LogicalSize::new(config.size.0, config.size.1))
            .with_resizable(config.resizable)
            .with_visible(false); // Must be invisible for AccessKit adapter creation

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
        // client-drawn (Wayland, X11). On Windows we keep
        // `with_decorations(true)` because the M4 recipe relies on the native
        // frame still being present (DwmExtendFrameIntoClientArea +
        // WM_NCCALCSIZE), and on macOS the M3 recipe sets the relevant
        // attributes via `WindowAttributesExtMacOS` — neither needs the toggle
        // here.
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
        // X11 / Wayland: `with_parent_window` is correct — winit wires
        // it through `WM_TRANSIENT_FOR` / `xdg_toplevel.set_parent`,
        // both of which match dialog semantics.
        //
        // macOS: skip here and defer to `attach_child_window` after
        // `PlatformWindow::new_with_a11y`. AppKit's
        // `-[NSWindow addChildWindow:ordered:]` orders the child
        // front (making it visible), which would race with the
        // AccessKit adapter that requires a hidden window at
        // construction.
        #[cfg(target_os = "windows")]
        if let Some(parent_id) = modal_parent
            && let Some(parent_winit) = self.winit_id_for_teksilo(parent_id)
            && let Some(parent_managed) = self.windows.get(&parent_winit)
            && let Ok(parent_handle) = parent_managed.platform_window.window().window_handle()
            && let winit::raw_window_handle::RawWindowHandle::Win32(win32) = parent_handle.as_raw()
        {
            use winit::platform::windows::WindowAttributesExtWindows;
            window_attrs = window_attrs.with_owner_window(win32.hwnd.get());
        }

        #[cfg(all(unix, not(target_os = "macos")))]
        if let Some(parent_id) = modal_parent
            && let Some(parent_winit) = self.winit_id_for_teksilo(parent_id)
            && let Some(parent_managed) = self.windows.get(&parent_winit)
            && let Ok(parent_handle) = parent_managed.platform_window.window().window_handle()
        {
            // SAFETY: the parent window is managed by the WindowManager
            // and remains alive for the lifetime of the child.
            window_attrs = unsafe { window_attrs.with_parent_window(Some(parent_handle.as_raw())) };
        }

        // Opt-in: consume a startup activation token from the environment so a
        // window spawned by another instance's "open in new window" comes up
        // focused on Wayland. No-op off Wayland/X11 or when the env is unset.
        if config.activate_from_env {
            window_attrs =
                teksilo_platform::window_activation::apply_creation_token(window_attrs, target);
        }

        let window = target
            .create_window(window_attrs)
            .expect("winit window creation failed");
        let winit_id = window.id();
        let scale_factor = window.scale_factor();

        let mut translation_state = TranslationState::new();
        translation_state.set_scale_factor(scale_factor);

        // Resolve the initial theme from ThemeMode before building the tree
        let initial_theme = match self.theme_mode {
            ThemeMode::Manual => self.theme.clone(),
            // OS-following modes carry the id "system" so a `ThemeSwitcher`
            // recognizes the active theme as "follow OS", not a fixed pick.
            ThemeMode::FollowSystem => match window.theme() {
                Some(winit::window::Theme::Dark) => teksilo_core::presets::intui::dark(),
                _ => teksilo_core::presets::intui::light(),
            }
            .with_id("system"),
            ThemeMode::Native => {
                let os = teksilo_platform::os_theme::query_os_theme_colors();
                let base = if os.color_scheme.is_dark() {
                    teksilo_core::presets::intui::dark()
                } else {
                    teksilo_core::presets::intui::light()
                };
                Theme {
                    colors: ColorTokens::from_os_colors(&os),
                    ..base
                }
                .with_id("system")
            }
        };
        // Update the shared theme so all subsequent windows use the same base
        if self.theme_mode != ThemeMode::Manual {
            self.theme = initial_theme.clone();
        }

        // Create with AccessKit adapter (shows window after adapter is ready)
        let mut pw = pollster::block_on(PlatformWindow::new_with_a11y(window, target));

        // macOS-only: the parent-child attach was deferred out of the
        // winit builder above to avoid the AppKit auto-show that races
        // with AccessKit adapter creation. Wire it now that the child
        // is visible.
        #[cfg(target_os = "macos")]
        if let Some(parent_id) = modal_parent
            && let Some(parent_winit) = self.winit_id_for_teksilo(parent_id)
            && let Some(parent_managed) = self.windows.get(&parent_winit)
        {
            teksilo_platform::attach_child_window(
                parent_managed.platform_window.window(),
                pw.window(),
            );
        }

        if is_modal {
            // No `set_window_level(AlwaysOnTop)` here — see the comment
            // on `with_window_level` above. The owner relationship
            // already keeps the modal ordered above its parent.
            teksilo_platform::window_activation::raise(pw.window(), None);
        }

        // Construct the platform title bar host if custom chrome was
        // requested. On unsupported platforms (X11, no host backend) the
        // factory logs a warning and returns `Unsupported`; we silently
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

        let mut tree = WidgetTree::new().with_theme(initial_theme);
        // Surface the window's HiDPI device scale to widgets that bridge to a
        // device-pixel OS resource (e.g. a `WebView` subview). Refreshed on
        // `ScaleFactorChanged`; the tree is otherwise fully logical.
        tree.set_device_scale_factor(scale_factor as f32);

        // Seed the tree from the active i18n manager (if any). Without
        // this, `WidgetTree::new()` defaults to `LayoutDirection::LeftToRight`
        // and an empty locale — so a windowed app started in an RTL
        // locale (via `TeksiloAppBuilder::i18n(...)` with Arabic/Hebrew as
        // initial) would lay out its first window as LTR until the
        // user manually triggered a locale switch. New windows created
        // mid-session also benefit: they inherit the active locale
        // and direction instead of reverting to the default.
        //
        // The seeding must happen BEFORE `root_builder` runs so any
        // `tr!` calls inside `build()` see the correct locale on
        // first build, and BEFORE the first layout pass so the tree's
        // `layout_direction` field already matches `m.direction_signal()`.
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
        if let Some(ref host) = title_bar_host {
            tree.set_title_bar_host(host.clone());
        }
        tree.set_accessibility_preferences(
            self.a11y_prefs.high_contrast,
            self.a11y_prefs.reduced_motion,
            self.a11y_prefs.text_scale_factor,
        );
        if (self.user_text_scale - 1.0).abs() > f32::EPSILON {
            tree.set_user_text_scale(self.user_text_scale);
        }

        #[cfg_attr(not(feature = "text"), allow(unused_mut))]
        let mut primed_atlas_version: u64 = 0;
        #[cfg(feature = "text")]
        {
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
        }

        // Attach this window's state to the tree so widgets can bind
        // against its own window signals via `ctx.window()`.
        tree.set_window_state(state.clone());

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
            if let Some(post_root) = config.take_post_root_builder() {
                root_id = post_root(&mut tree, root_id);
            } else if !is_modal
                && let Some(default_post_root) = self
                    .app_context_template
                    .as_ref()
                    .and_then(|t| t.app_state::<crate::DefaultPostRoot>().cloned())
            {
                root_id = (default_post_root.0)(&mut tree, root_id);
            }

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
            last_autosize_height: None,
            title_bar_host,
            focused: true,
            occluded: false,
            caps_lock_active: false,
            ime_allowed: None,
            ime_purpose: None,
            last_ime_preedit_empty: false,
            _persist_handles: persist_handles,
            close_guard,
            can_close,
            on_close_blocked,
            on_removed,
            atlas_uploaded_version: primed_atlas_version,
        };

        self.windows.insert(winit_id, managed);
        self.teksilo_to_winit.insert(teksilo_id, winit_id);

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
    pub fn close_window(&mut self, teksilo_id: TeksiloWindowId) {
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
        if let Some(winit_id) = self.teksilo_to_winit.remove(&teksilo_id)
            && let Some(mut managed) = self.windows.remove(&winit_id)
        {
            // If this window is involved in an in-flight app-originated OS
            // drag, abort it before the tree is dropped — otherwise the
            // app-global typed-payload stash would leak and a later genuine
            // external drop could be misrecovered as the stale payload.
            managed.tree.abort_outbound_drag();
            if let Some(sid) = managed.string_id.as_deref() {
                self.string_to_id.remove(sid);
            }
            // Unblock parent if this was a modal
            if managed.modal
                && let Some(parent_id) = managed.parent
            {
                self.modal_blocked.remove(&parent_id);
            }
            if let Some(hook) = managed.on_removed.take() {
                removed_hook = Some((hook, managed.string_id.clone()));
            }
        }
        // Also remove any modal children blocking this window
        self.modal_blocked.remove(&teksilo_id);

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
    /// window open. Used for the interactive close gestures: the OS
    /// close button, a custom-chrome close button, and
    /// [`EventContext::close_window`](teksilo_core::widget::EventContext::close_window).
    pub fn request_close(&mut self, teksilo_id: TeksiloWindowId) {
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
            let Some(managed) = self.windows.get(&winit_id) else {
                continue;
            };
            apply_window_command(managed.platform_window.window(), cmd);
        }
    }

    /// Process pending window closures. Called from the event loop
    /// each tick. Creation does not need a drain path — `open_window`
    /// from handler code goes through [`WindowOpsImpl`] and calls
    /// [`create_window`](Self::create_window) synchronously inside the
    /// same dispatch.
    ///
    /// A *forced* close ([`queue_close`](Self::queue_close)) tears the
    /// window down immediately. A *guarded* close
    /// ([`request_close`](Self::request_close)) first runs the window's
    /// close guard via `evaluate_close_guard`; a
    /// [`CloseResponse::Veto`](teksilo_core::CloseResponse) keeps the
    /// window open. Guards are strictly per-window, so this is correct
    /// for multi-window apps: each pending close consults only its own
    /// window's guard.
    pub fn process_pending(&mut self, target: &winit::event_loop::ActiveEventLoop) {
        let closes = std::mem::take(&mut self.pending_closes);
        for pending in closes {
            if pending.force || self.evaluate_close_guard(pending.id, target) {
                self.close_window(pending.id);
            }
            // Vetoed guarded close → the window stays open. The app may
            // have opened a confirmation dialog from inside the guard;
            // its "close anyway" button calls
            // `EventContext::close_window_forced`, which re-queues this
            // window as a forced close on a later tick.
        }
    }

    /// Run the close guard for `teksilo_id` (if it declared one) and
    /// return whether the close should proceed.
    ///
    /// A window with no guard returns `true` without building an
    /// `EventContext`. Otherwise the guard runs with a real
    /// [`EventContext`](teksilo_core::widget::EventContext) for the
    /// window's own tree (so it can open a confirmation dialog, set
    /// signals, fire intents…), and the verdict is:
    ///
    /// 1. [`can_close`](teksilo_core::WindowConfig::can_close) sugar: a
    ///    `false` signal vetoes and fires
    ///    [`on_close_blocked`](teksilo_core::WindowConfig::on_close_blocked).
    /// 2. otherwise the
    ///    [`on_close_requested`](teksilo_core::WindowConfig::on_close_requested)
    ///    guard's [`CloseResponse`].
    /// 3. otherwise `true` (close).
    ///
    /// Strictly per-window — only `teksilo_id`'s own guard is consulted.
    fn evaluate_close_guard(
        &mut self,
        teksilo_id: TeksiloWindowId,
        target: &winit::event_loop::ActiveEventLoop,
    ) -> bool {
        let Some(&winit_id) = self.teksilo_to_winit.get(&teksilo_id) else {
            // Unknown / already-gone window — let `close_window` no-op.
            return true;
        };
        // Fast path: no guard configured → close immediately, without
        // paying for an EventContext.
        match self.windows.get(&winit_id) {
            Some(managed) if managed.close_guard.is_none() && managed.can_close.is_none() => {
                return true;
            }
            None => return true,
            _ => {}
        }
        // Take the window out of the map so we can borrow `&mut self`
        // for `WindowOpsImpl` while still holding the window's tree.
        let Some(mut managed) = self.take_managed(winit_id) else {
            return true;
        };

        #[cfg(not(target_os = "macos"))]
        let current_handle = managed
            .platform_window
            .window()
            .window_handle()
            .ok()
            .map(|h| h.as_raw());
        let current_arc = Some(managed.platform_window.window_arc());

        // Clone the guard handles so the dispatch closure captures them
        // disjointly from `managed.tree`, which `run_with_event_context`
        // borrows mutably.
        let close_guard = managed.close_guard.clone();
        let can_close = managed.can_close.clone();
        let on_close_blocked = managed.on_close_blocked.clone();

        let mut should_close = true;
        {
            let mut ops = WindowOpsImpl::new(
                self,
                target,
                teksilo_id,
                #[cfg(not(target_os = "macos"))]
                current_handle,
                current_arc,
            );
            managed.tree.run_with_event_context(&mut ops, |ctx| {
                should_close = close_verdict(&can_close, &on_close_blocked, &close_guard, ctx);
            });
        }

        self.reinsert_managed(winit_id, managed);
        should_close
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
        self.windows.remove(&id)
    }

    /// Re-insert a `ManagedWindow` previously extracted via
    /// [`take_managed`](Self::take_managed).
    pub(crate) fn reinsert_managed(&mut self, id: winit::window::WindowId, managed: ManagedWindow) {
        self.windows.insert(id, managed);
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

    pub fn blocking_modal_child(&self, teksilo_id: TeksiloWindowId) -> Option<TeksiloWindowId> {
        self.modal_blocked.get(&teksilo_id).copied()
    }

    pub fn refocus_modal_child(&self, blocked_parent: TeksiloWindowId) {
        let Some(child_id) = self.blocking_modal_child(blocked_parent) else {
            return;
        };
        let Some(child_winit) = self.winit_id_for_teksilo(child_id) else {
            return;
        };
        let Some(child) = self.windows.get(&child_winit) else {
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
        teksilo_platform::window_activation::raise(child.platform_window.window(), None);
        child.platform_window.request_redraw();
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
        for managed in self.windows.values_mut() {
            managed.tree.set_theme(theme.clone());
        }
    }

    /// Broadcast a user text-scale change to all windows. Stores the factor so
    /// windows created later inherit it, then re-scales every existing tree's
    /// text without rebuilding.
    pub fn set_text_scale(&mut self, factor: f32) {
        self.user_text_scale = factor;
        for managed in self.windows.values_mut() {
            managed.tree.set_user_text_scale(factor);
        }
    }

    /// Seed the user text-scale factor before the first window opens. Called by
    /// `TeksiloAppHandler` after reading `teksilo_settings::TEXT_SCALE_KEY`, so
    /// every initially-created tree starts at the persisted scale.
    pub fn set_initial_text_scale(&mut self, factor: f32) {
        self.user_text_scale = factor;
    }

    /// Re-query the OS accessibility preferences ("increase contrast", "reduce
    /// motion", text scale) and, if they changed since startup / the last
    /// refresh, apply them to every open window's tree. Lets a runtime toggle
    /// of these settings take effect without restarting the app (WCAG / EN
    /// 301 549 §11.7). Driven event-first — from `WindowEvent::Focused` when a
    /// window gains focus — so there is no idle polling wakeup. Returns `true`
    /// if anything changed (so the caller can request a redraw).
    pub fn refresh_accessibility_preferences(&mut self) -> bool {
        let fresh = AccessibilityPreferences::query();
        if fresh == self.a11y_prefs {
            return false;
        }
        let hc = fresh.high_contrast;
        let rm = fresh.reduced_motion;
        let ts = fresh.text_scale_factor;
        self.a11y_prefs = fresh;
        for managed in self.windows.values_mut() {
            managed.tree.set_accessibility_preferences(hc, rm, ts);
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
        for managed in self.windows.values_mut() {
            if outcome.direction_changed {
                managed.tree.set_layout_direction(new_dir);
            }
            managed.tree.set_locale(locale.to_string());
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

    /// Number of active windows.
    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    /// Whether no windows remain (app should exit).
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

    /// Iterate the `TeksiloWindowId` of every managed window. Feeds the
    /// automation `list_windows` tool (debug-only `automation` feature),
    /// which pairs each id with its `string_id` label and current title.
    #[allow(dead_code)]
    pub(crate) fn teksilo_ids(&self) -> impl Iterator<Item = TeksiloWindowId> + '_ {
        self.windows.values().map(|m| m.teksilo_id)
    }

    /// Get the winit WindowId for a TeksiloWindowId.
    pub fn winit_id_for_teksilo(
        &self,
        teksilo_id: TeksiloWindowId,
    ) -> Option<winit::window::WindowId> {
        self.teksilo_to_winit.get(&teksilo_id).copied()
    }

    /// Get the platform title bar host for a window, if the window opted
    /// into custom chrome via `WindowConfig::custom_chrome(true)` and the
    /// platform supports it. Returns `None` for windows that use native
    /// decorations or run on a window system without custom chrome support
    /// (currently X11).
    pub fn title_bar_host(
        &self,
        teksilo_id: TeksiloWindowId,
    ) -> Option<Rc<dyn PlatformTitleBarHost>> {
        let winit_id = self.teksilo_to_winit.get(&teksilo_id).copied()?;
        self.windows
            .get(&winit_id)
            .and_then(|w| w.title_bar_host.clone())
    }

    /// Request redraw on all windows.
    pub fn request_redraw_all(&self) {
        for managed in self.windows.values() {
            managed.platform_window.request_redraw();
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
    pub fn request_redraw_due(&self, now: std::time::Instant) {
        for managed in self.windows.values() {
            if managed
                .tree
                .next_timer_deadline()
                .is_some_and(|deadline| deadline <= now)
            {
                managed.platform_window.request_redraw();
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
        let mut poked = 0;
        for managed in self.windows.values_mut() {
            if managed.tree.needs_reconcile() {
                let size = managed.platform_window.surface_size();
                let sf = managed.platform_window.scale_factor() as f32;
                let proposal =
                    teksilo_canvas::SizeProposal::exact(size.0 as f32 / sf, size.1 as f32 / sf);
                managed.tree.layout(proposal);
            }
            if managed.tree.needs_render() {
                managed.platform_window.request_redraw();
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

    /// Drain native modal-window dismiss requests from all windows.
    pub fn drain_pending_modal_dismissals(&mut self) -> Vec<TeksiloWindowId> {
        let mut windows_to_close = Vec::new();
        for managed in self.windows.values_mut() {
            if managed.tree.drain_pending_modal_dismissal() && managed.modal {
                windows_to_close.push(managed.teksilo_id);
            }
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
        for managed in self.windows.values_mut() {
            if let Some(loc) = managed.tree.take_pending_locale_request() {
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
        for managed in self.windows.values_mut() {
            if let Some(theme) = managed.tree.take_pending_theme_request() {
                requests.push(theme);
            }
        }
        let had_requests = !requests.is_empty();
        for theme in requests {
            self.set_theme(theme);
        }
        if had_requests {
            // An explicit theme pick disables OS-following so a later OS
            // light/dark change won't override the chosen theme. (The
            // internal `apply_os_theme` path calls `set_theme` directly, not
            // through this drain, so it is unaffected.)
            self.theme_mode = ThemeMode::Manual;
        }
        had_requests
    }

    /// Drain per-tree "follow OS theme" requests raised by handlers via
    /// [`EventContext::follow_system_theme`](teksilo_core::widget::EventContext::follow_system_theme).
    /// Switches the app to [`ThemeMode::Native`] and recomputes the theme from
    /// the current OS colours, fanning it to every window. Returns `true` if
    /// any window requested it (so the caller schedules a repaint).
    pub fn drain_pending_follow_system_requests(&mut self) -> bool {
        let mut requested = false;
        for managed in self.windows.values_mut() {
            if managed.tree.take_pending_follow_system_request() {
                requested = true;
            }
        }
        if requested {
            self.theme_mode = ThemeMode::Native;
            // No winit hint at request time; apply_os_theme falls back to the
            // current window's reported theme.
            self.apply_os_theme(None);
        }
        requested
    }

    /// Drain per-tree text-scale requests raised by handlers via
    /// [`EventContext::set_text_scale`](teksilo_core::widget::EventContext::set_text_scale)
    /// and route each through [`WindowManager::set_text_scale`] so the new
    /// factor is applied to *every* window. Returns `true` if any request was
    /// drained (so the caller schedules a repaint). Last writer wins if several
    /// windows requested in the same tick.
    pub fn drain_pending_text_scale_requests(&mut self) -> bool {
        let mut requests: Vec<f32> = Vec::new();
        for managed in self.windows.values_mut() {
            if let Some(scale) = managed.tree.take_pending_text_scale_request() {
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

/// Translate a [`WindowCommand`] into the appropriate winit call.
///
/// `Close` is handled elsewhere (see [`WindowManager::drain_window_commands`]).
fn apply_window_command(win: &winit::window::Window, cmd: WindowCommand) {
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
            let _ = win.request_inner_size(winit::dpi::LogicalSize::new(w, h));
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
            teksilo_platform::window_activation::raise(win, activation_token.as_deref());
        }
        WindowCommand::Close => {
            // Handled in drain_window_commands; unreachable here.
        }
    }
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
    /// Current (dispatching) window's id. Kept for diagnostics and
    /// future modal-parent self-reference logic.
    current_id: TeksiloWindowId,
    /// Current window's raw handle, captured before removal so a
    /// modal whose parent is the current window can still attach.
    #[cfg(not(target_os = "macos"))]
    current_handle: Option<winit::raw_window_handle::RawWindowHandle>,
    /// `Arc<Window>` of the current (dispatching) window. On macOS
    /// it is needed for `addChildWindow:ordered:`. On every platform
    /// it backs `current_parent_handle()` so native-dialog
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
        #[cfg(not(target_os = "macos"))] current_handle: Option<
            winit::raw_window_handle::RawWindowHandle,
        >,
        current_window_arc: Option<std::sync::Arc<winit::window::Window>>,
    ) -> Self {
        Self {
            wm,
            event_loop,
            current_id,
            #[cfg(not(target_os = "macos"))]
            current_handle,
            current_window_arc,
        }
    }
}

impl teksilo_core::WindowOps for WindowOpsImpl<'_> {
    fn open_window(&mut self, config: teksilo_core::WindowConfig) -> TeksiloWindowId {
        let _ = self.current_id;
        #[cfg(not(target_os = "macos"))]
        let _ = self.current_handle;
        #[cfg(target_os = "macos")]
        let _ = &self.current_window_arc;
        self.wm.create_window(config, self.event_loop)
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
        if let Some(winit_id) = self.wm.teksilo_to_winit_map().get(&id).copied()
            && let Some(managed) = self.wm.windows_map().get(&winit_id)
        {
            teksilo_platform::window_activation::raise(managed.platform_window.window(), None);
        }
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
        handle.begin_drag(self.current_id, &data, image.as_ref())
    }

    fn cancel_os_drag(&mut self) {
        use teksilo_platform::external_dnd::ExternalDndHandle;
        if let Some(handle) = self
            .wm
            .app_context_template()
            .and_then(|t| t.app_state::<ExternalDndHandle>().cloned())
        {
            handle.cancel_drag(self.current_id);
        }
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
            let col = ctx.add(VStack::new().add_child(first).add_child(second));
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
    /// return its boolean verdict. Mirrors the `run_with_event_context`
    /// path `process_pending` uses, but with a `NoopWindowOps` sink and
    /// no winit event loop.
    fn verdict(
        can_close: Option<Prop<bool>>,
        on_blocked: Option<CloseBlockedCallback>,
        guard: Option<CloseGuard>,
    ) -> bool {
        let mut tree = WidgetTree::new();
        let mut out = true;
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

    #[test]
    fn can_close_false_vetoes_and_fires_blocked() {
        let fired = Rc::new(Cell::new(false));
        let flag = fired.clone();
        let on_blocked: CloseBlockedCallback = Rc::new(move |_ctx| flag.set(true));

        let should_close = verdict(Some(Prop::from(Signal::new(false))), Some(on_blocked), None);

        assert!(!should_close, "can_close == false must veto");
        assert!(fired.get(), "on_close_blocked must fire on a vetoed close");
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
        let on_blocked: CloseBlockedCallback = Rc::new(move |_ctx| flag.set(true));

        let should_close = verdict(Some(Prop::from(Signal::new(true))), Some(on_blocked), None);

        assert!(should_close, "a permissive signal with no guard closes");
        assert!(!fired.get(), "on_close_blocked must not fire when allowed");
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
