//! Multi-window management.
//!
//! The `WindowManager` maintains a collection of active windows, each with its
//! own `WidgetTree`, `PlatformWindow`, and translation state. It routes events
//! by winit `WindowId`, broadcasts environment changes to all windows, and
//! handles modal dialog blocking.

use std::collections::HashMap;
use std::rc::Rc;

use bastyde_core::Theme;
use bastyde_core::event_source::TreeAppContext;
use bastyde_core::{
    DecorationsMode, PlatformTitleBarHost, TitleBarHostCallbacks, UserAttentionKind, WidgetTree,
    WindowCommand, WindowPlacement, WindowState, WindowStateInit,
};
use bastyde_platform::AccessibilityPreferences;
use bastyde_platform::PlatformWindow;
use bastyde_platform::create_title_bar_host;
use bastyde_platform::event_translation::TranslationState;
use bastyde_tokens::ColorTokens;
#[allow(unused_imports)]
use winit::raw_window_handle::HasWindowHandle;
use winit::window::UserAttentionType;
use winit::window::WindowLevel;

use crate::app::{AppEventProxy, CloseWindowRequest, ThemeMode};

use crate::window_config::{BastydeWindowId, WindowConfig};

/// Per-window state managed by the WindowManager.
pub(crate) struct ManagedWindow {
    pub bastyde_id: BastydeWindowId,
    pub string_id: Option<String>,
    pub tree: WidgetTree,
    /// Reactive per-window state shared with the `WidgetTree` and
    /// accessible from handlers via
    /// [`EventContext::window`](bastyde_core::widget::EventContext::window).
    pub state: WindowState,
    pub platform_window: PlatformWindow,
    pub translation_state: TranslationState,
    pub current_modifiers: winit::keyboard::ModifiersState,
    pub modal: bool,
    pub parent: Option<BastydeWindowId>,
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
    /// RAII handles for the auto-save observers wired to
    /// `state.{size, position, placement}` when a
    /// `WindowStateService` is registered. Dropped when the window
    /// is removed from `WindowManager::windows`.
    pub _persist_handles: Vec<bastyde_core::ObserverHandle>,
}

/// Manages multiple application windows.
///
/// Each window owns its own `WidgetTree` and `PlatformWindow`. The manager
/// routes events by winit `WindowId`, broadcasts environment changes, and
/// handles modal dialog blocking.
pub struct WindowManager {
    windows: HashMap<winit::window::WindowId, ManagedWindow>,
    bastyde_to_winit: HashMap<BastydeWindowId, winit::window::WindowId>,
    /// Stable string-id → id lookup, populated whenever a config carries
    /// `id(...)`. Used by `WindowOps::find_window`.
    string_to_id: HashMap<String, BastydeWindowId>,
    /// Next allocatable `BastydeWindowId`. Bumped by `alloc_id`; never
    /// reused after a window closes.
    next_id: u64,
    /// Pending close requests collected from handler code (via
    /// `EventContext::close_window` / `close_window_by_id`). Drained
    /// once per tick in [`process_pending`](Self::process_pending).
    pending_closes: Vec<BastydeWindowId>,
    theme: Theme,
    #[cfg(feature = "text")]
    typesetter: Option<bastyde_text::SharedTypesetter>,
    /// Windows that are blocked by a modal child.
    modal_blocked: HashMap<BastydeWindowId, BastydeWindowId>,
    /// OS-level accessibility preferences, queried once at startup.
    a11y_prefs: AccessibilityPreferences,
    /// How the app resolves its theme (Manual, FollowSystem, Native).
    theme_mode: ThemeMode,
    /// Per-tree app context shared with every window's WidgetTree when an
    /// event source is registered on the BastydeAppBuilder. Each window
    /// receives a clone of this Rc so subscriptions land in a single
    /// shared `subscription_callbacks` map.
    app_context_template: Option<Rc<TreeAppContext>>,
    /// Event-loop proxy used to construct `TitleBarHostCallbacks` when a
    /// window opts into custom chrome. Installed by `BastydeAppHandler::new`
    /// after the proxy is minted in `BastydeAppBuilder::run`. `None` during
    /// tests or the headless path, in which case the host's `close()`
    /// callback is a no-op (`TitleBarHostCallbacks::noop`).
    event_proxy: Option<AppEventProxy>,
}

impl WindowManager {
    pub fn new(theme: Theme) -> Self {
        let a11y_prefs = AccessibilityPreferences::query();
        Self {
            windows: HashMap::new(),
            bastyde_to_winit: HashMap::new(),
            string_to_id: HashMap::new(),
            next_id: 1,
            pending_closes: Vec::new(),
            theme,
            #[cfg(feature = "text")]
            typesetter: None,
            modal_blocked: HashMap::new(),
            a11y_prefs,
            theme_mode: ThemeMode::Manual,
            app_context_template: None,
            event_proxy: None,
        }
    }

    /// Set the theme mode (called by BastydeAppHandler during initialization).
    pub fn set_theme_mode(&mut self, mode: ThemeMode) {
        self.theme_mode = mode;
    }

    /// Install the event-loop proxy (called by BastydeAppHandler once the
    /// proxy is available). Enables `TitleBarHostCallbacks::request_close`
    /// to post `CloseWindowRequest` back through the event loop.
    pub fn set_event_proxy(&mut self, proxy: AppEventProxy) {
        self.event_proxy = Some(proxy);
    }

    /// Install the per-tree app context template that every newly created
    /// window's WidgetTree should adopt. Called by BastydeAppHandler when the
    /// application registered an event source on the builder.
    pub fn set_app_context_template(&mut self, template: Rc<TreeAppContext>) {
        self.app_context_template = Some(template);
    }

    /// The shared per-tree app context, if any, used by BastydeAppHandler to
    /// look up subscription callbacks when delivering
    /// `AppEvent::SubscriptionEvent` to the UI thread.
    pub(crate) fn app_context_template(&self) -> Option<&Rc<TreeAppContext>> {
        self.app_context_template.as_ref()
    }

    /// Get the current theme mode.
    pub fn theme_mode(&self) -> ThemeMode {
        self.theme_mode
    }

    #[cfg(feature = "text")]
    pub fn set_typesetter(&mut self, typesetter: bastyde_text::SharedTypesetter) {
        self.typesetter = Some(typesetter);
    }

    fn alloc_id(&mut self) -> BastydeWindowId {
        let id = BastydeWindowId::new(self.next_id);
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
    ) -> BastydeWindowId {
        // If a `WindowStateService` is registered AND this window has
        // a stable `id(...)`, restore the saved geometry — sanitized
        // against the current monitor — into `config` before any
        // winit attribute is built. See `window_persist` for the
        // exact policy.
        let persist_service: Option<bastyde_settings::WindowStateService> = self
            .app_context_template
            .as_ref()
            .and_then(|t| t.app_state::<bastyde_settings::WindowStateService>().cloned());
        if let Some(svc) = persist_service.as_ref() {
            crate::window_persist::apply_restored_geometry(&mut config, svc, target);
        }

        let bastyde_id = self.alloc_id();
        let state = WindowState::new(WindowStateInit {
            id: bastyde_id,
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
            self.string_to_id.insert(sid.clone(), bastyde_id);
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
            && let Some(parent_winit) = self.winit_id_for_bastyde(parent_id)
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
                        "bastyde-app: failed to build window icon ({}×{}): {e}",
                        icon.width, icon.height
                    ),
                }
            } else {
                eprintln!(
                    "bastyde-app: window icon buffer size ({}) does not match {}×{}×4 ({}); \
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
        // client-drawn (Wayland). On Windows we keep `with_decorations(true)`
        // because the M4 recipe relies on the native frame still being present
        // (DwmExtendFrameIntoClientArea + WM_NCCALCSIZE), and on macOS the
        // M3 recipe sets the relevant attributes via `WindowAttributesExtMacOS`
        // — neither needs the toggle here.
        #[cfg(all(unix, not(target_os = "macos")))]
        if wants_custom_chrome
            && bastyde_platform::active_window_system() == bastyde_platform::WindowSystem::Wayland
        {
            window_attrs = window_attrs.with_decorations(false);
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
            && let Some(parent_winit) = self.winit_id_for_bastyde(parent_id)
            && let Some(parent_managed) = self.windows.get(&parent_winit)
            && let Ok(parent_handle) = parent_managed.platform_window.window().window_handle()
            && let winit::raw_window_handle::RawWindowHandle::Win32(win32) = parent_handle.as_raw()
        {
            use winit::platform::windows::WindowAttributesExtWindows;
            window_attrs = window_attrs.with_owner_window(win32.hwnd.get());
        }

        #[cfg(all(unix, not(target_os = "macos")))]
        if let Some(parent_id) = modal_parent
            && let Some(parent_winit) = self.winit_id_for_bastyde(parent_id)
            && let Some(parent_managed) = self.windows.get(&parent_winit)
            && let Ok(parent_handle) = parent_managed.platform_window.window().window_handle()
        {
            // SAFETY: the parent window is managed by the WindowManager
            // and remains alive for the lifetime of the child.
            window_attrs = unsafe { window_attrs.with_parent_window(Some(parent_handle.as_raw())) };
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
            ThemeMode::FollowSystem => match window.theme() {
                Some(winit::window::Theme::Dark) => bastyde_core::presets::intui::dark(),
                _ => bastyde_core::presets::intui::light(),
            },
            ThemeMode::Native => {
                let os = bastyde_platform::os_theme::query_os_theme_colors();
                let base = if os.color_scheme.is_dark() {
                    bastyde_core::presets::intui::dark()
                } else {
                    bastyde_core::presets::intui::light()
                };
                Theme {
                    colors: ColorTokens::from_os_colors(&os),
                    ..base
                }
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
            && let Some(parent_winit) = self.winit_id_for_bastyde(parent_id)
            && let Some(parent_managed) = self.windows.get(&parent_winit)
        {
            bastyde_platform::attach_child_window(
                parent_managed.platform_window.window(),
                pw.window(),
            );
        }

        if is_modal {
            // No `set_window_level(AlwaysOnTop)` here — see the comment
            // on `with_window_level` above. The owner relationship
            // already keeps the modal ordered above its parent.
            pw.window().focus_window();
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
                            close_proxy.send_external(CloseWindowRequest { bastyde_id });
                        }),
                        // Used by the Windows backend to post
                        // `TitleBarSyntheticEvent` / `TitleBarHoverEvent`
                        // back through `AppEvent::External`. Wayland
                        // and macOS construct the host but never call
                        // this closure.
                        post_external: Rc::new(move |payload| {
                            post_proxy.send_external_boxed(payload);
                        }),
                        bastyde_id,
                    }
                }
                // Headless / test path: no event loop proxy is installed, so
                // the host's close() becomes a silent no-op. Real windowed
                // runs always install a proxy via `set_event_proxy`.
                None => TitleBarHostCallbacks {
                    bastyde_id,
                    ..TitleBarHostCallbacks::noop()
                },
            };
            create_title_bar_host(pw.window_arc(), callbacks).ok()
        } else {
            None
        };

        let mut tree = WidgetTree::new().with_theme(initial_theme);

        // Seed the tree from the active i18n manager (if any). Without
        // this, `WidgetTree::new()` defaults to `LayoutDirection::LeftToRight`
        // and an empty locale — so a windowed app started in an RTL
        // locale (via `BastydeAppBuilder::i18n(...)` with Arabic/Hebrew as
        // initial) would lay out its first window as LTR until the
        // user manually triggered a locale switch. New windows created
        // mid-session also benefit: they inherit the active locale
        // and direction instead of reverting to the default.
        //
        // The seeding must happen BEFORE `root_builder` runs so any
        // `tr!` calls inside `build()` see the correct locale on
        // first build, and BEFORE the first layout pass so the tree's
        // `layout_direction` field already matches `m.direction_signal()`.
        if let Some((loc, dir)) = bastyde_i18n::thread_local::with_active(|m| {
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

        #[cfg(feature = "text")]
        {
            if let Some(ref typesetter) = self.typesetter {
                typesetter.set_scale_factor(scale_factor as f32);
                tree = tree.with_text_backend(typesetter.as_text_backend());

                // Prime the new window's GPU atlas from the shared
                // typesetter. The dirty-flag path in `handle_redraw_requested`
                // only uploads when `atlas_info()` reports pending changes;
                // a window created after the atlas already contains every
                // glyph it needs (e.g. reopening a modal with the same
                // labels) would otherwise render text against an empty
                // per-window atlas texture.
                let (w, h, pixels) = {
                    let bridge = typesetter.bridge().borrow();
                    let service = bridge.service();
                    (
                        service.atlas_width(),
                        service.atlas_height(),
                        service.atlas_pixels().to_vec(),
                    )
                };
                if w > 0 && h > 0 {
                    pw.renderer_mut().upload_atlas(w, h, &pixels);
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
            if let Some(post_root) = config.take_post_root_builder() {
                root_id = post_root(&mut tree, root_id);
            } else if let Some(default_post_root) = self
                .app_context_template
                .as_ref()
                .and_then(|t| t.app_state::<crate::DefaultPostRoot>().cloned())
            {
                root_id = (default_post_root.0)(&mut tree, root_id);
            }

            if is_modal {
                let focus_target = modal_focus_target
                    .filter(|id| tree.is_active(*id))
                    .or_else(|| tree.widget_initial_focus_hint(root_id))
                    .or_else(|| tree.first_focusable_descendant(root_id));
                if let Some(id) = focus_target {
                    tree.focus(id);
                }
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
            self.modal_blocked.insert(parent_id, bastyde_id);
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
            bastyde_id,
            string_id: config.string_id,
            tree,
            state,
            platform_window: pw,
            translation_state,
            current_modifiers: winit::keyboard::ModifiersState::empty(),
            modal: is_modal,
            parent: modal_parent,
            title_bar_host,
            focused: true,
            occluded: false,
            caps_lock_active: false,
            _persist_handles: persist_handles,
        };

        self.windows.insert(winit_id, managed);
        self.bastyde_to_winit.insert(bastyde_id, winit_id);

        bastyde_id
    }

    /// Close a window by its BastydeWindowId.
    pub fn close_window(&mut self, bastyde_id: BastydeWindowId) {
        // Purge any pending file-dialog callbacks owned by the
        // soon-to-close window before its tree is dropped — see
        // `bastyde_platform::file_dialog::FileDialogHandle::purge_window`.
        // A worker-thread future that resolves after this point will
        // still arrive at the dispatcher; deliver finds no pending
        // entry and silently drops.
        #[cfg(feature = "file-dialog")]
        {
            if let Some(handle) = self
                .app_context_template
                .as_ref()
                .and_then(|t| t.app_state::<bastyde_platform::file_dialog::FileDialogHandle>())
            {
                handle.purge_window(bastyde_id);
            }
        }
        if let Some(winit_id) = self.bastyde_to_winit.remove(&bastyde_id)
            && let Some(managed) = self.windows.remove(&winit_id)
        {
            if let Some(sid) = managed.string_id.as_deref() {
                self.string_to_id.remove(sid);
            }
            // Unblock parent if this was a modal
            if managed.modal
                && let Some(parent_id) = managed.parent
            {
                self.modal_blocked.remove(&parent_id);
            }
        }
        // Also remove any modal children blocking this window
        self.modal_blocked.remove(&bastyde_id);
    }

    /// Queue a window closure (processed in the next event loop tick).
    pub fn queue_close(&mut self, bastyde_id: BastydeWindowId) {
        self.pending_closes.push(bastyde_id);
    }

    /// Route a Windows-side synthetic title-bar tap. The wndproc
    /// posts a [`crate::TitleBarSyntheticEvent`] when `WM_NCLBUTTONUP`
    /// fires on a button rect that the OS treated as non-client; we
    /// resolve the matching `WidgetId` via the host and synthesise a
    /// primary-button tap so the widget's normal `on_tap` handler
    /// runs. No-op on platforms that never produce these events.
    pub fn route_title_bar_synthetic_tap(
        &mut self,
        bastyde_id: BastydeWindowId,
        target: bastyde_core::ControlTarget,
    ) {
        let Some(winit_id) = self.bastyde_to_winit.get(&bastyde_id).copied() else {
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
        bastyde_id: BastydeWindowId,
        target: bastyde_core::ControlTarget,
        entered: bool,
    ) {
        let Some(winit_id) = self.bastyde_to_winit.get(&bastyde_id).copied() else {
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
        let mut batch: Vec<(winit::window::WindowId, BastydeWindowId, WindowCommand)> = Vec::new();
        for (winit_id, managed) in self.windows.iter() {
            for cmd in managed.state.drain_os_commands() {
                batch.push((*winit_id, managed.bastyde_id, cmd));
            }
        }
        for (winit_id, bastyde_id, cmd) in batch {
            // `Close` is the one command that needs to mutate
            // `self.windows` — queue it for the tick-end close drain
            // instead of running it inline.
            if matches!(cmd, WindowCommand::Close) {
                self.pending_closes.push(bastyde_id);
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
    pub fn process_pending(&mut self, _target: &winit::event_loop::ActiveEventLoop) {
        let closes: Vec<_> = self.pending_closes.drain(..).collect();
        for bastyde_id in closes {
            self.close_window(bastyde_id);
        }
    }

    /// Get a mutable ManagedWindow for a winit WindowId.
    pub(crate) fn get_by_winit_mut(
        &mut self,
        id: winit::window::WindowId,
    ) -> Option<&mut ManagedWindow> {
        self.windows.get_mut(&id)
    }

    /// Temporarily remove a managed window from the map. Used by
    /// [`BastydeAppHandler::dispatch_in_window`] so the handler's
    /// `&mut tree` borrow does not collide with
    /// [`WindowOpsImpl`](WindowOpsImpl)'s `&mut WindowManager` borrow.
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

    /// `pub(crate)` access to the windows map used by
    /// [`WindowOpsImpl`](WindowOpsImpl).
    pub(crate) fn windows_map(&self) -> &HashMap<winit::window::WindowId, ManagedWindow> {
        &self.windows
    }

    /// `pub(crate)` access to the bastyde→winit id map used by
    /// [`WindowOpsImpl`](WindowOpsImpl).
    pub(crate) fn bastyde_to_winit_map(&self) -> &HashMap<BastydeWindowId, winit::window::WindowId> {
        &self.bastyde_to_winit
    }

    pub(crate) fn get_by_bastyde_mut(&mut self, id: BastydeWindowId) -> Option<&mut ManagedWindow> {
        let winit_id = self.bastyde_to_winit.get(&id).copied()?;
        self.windows.get_mut(&winit_id)
    }

    /// Get the BastydeWindowId for a winit WindowId.
    pub fn bastyde_id_for_winit(&self, id: winit::window::WindowId) -> Option<BastydeWindowId> {
        self.windows.get(&id).map(|w| w.bastyde_id)
    }

    /// Find a window by its string ID.
    pub fn find_window(&self, string_id: &str) -> Option<BastydeWindowId> {
        self.string_to_id.get(string_id).copied()
    }

    /// Whether a window is blocked by a modal child.
    pub fn is_blocked(&self, bastyde_id: BastydeWindowId) -> bool {
        self.modal_blocked.contains_key(&bastyde_id)
    }

    pub fn blocking_modal_child(&self, bastyde_id: BastydeWindowId) -> Option<BastydeWindowId> {
        self.modal_blocked.get(&bastyde_id).copied()
    }

    pub fn refocus_modal_child(&self, blocked_parent: BastydeWindowId) {
        let Some(child_id) = self.blocking_modal_child(blocked_parent) else {
            return;
        };
        let Some(child_winit) = self.winit_id_for_bastyde(child_id) else {
            return;
        };
        let Some(child) = self.windows.get(&child_winit) else {
            return;
        };

        child
            .platform_window
            .window()
            .set_window_level(WindowLevel::AlwaysOnTop);
        child.platform_window.window().focus_window();
        child
            .platform_window
            .window()
            .request_user_attention(Some(UserAttentionType::Informational));
        child.platform_window.request_redraw();
    }

    /// Broadcast a theme change to all windows.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme.clone();
        for managed in self.windows.values_mut() {
            managed.tree.set_theme(theme.clone());
        }
    }

    /// Broadcast a locale switch to all windows. Updates the i18n manager
    /// (incrementing the version signal) and seeds each tree with the new
    /// locale and layout direction. No-op if no `I18nConfig` was registered.
    pub fn set_locale(&mut self, locale: bastyde_i18n::LanguageIdentifier) {
        let Some((outcome, new_dir)) = bastyde_i18n::thread_local::with_active(|mgr| {
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

    /// Get the BastydeWindowId of the first (primary) window.
    /// Falls back to a synthetic ID when no windows are open yet.
    pub fn primary_window_id(&self) -> BastydeWindowId {
        self.bastyde_to_winit
            .keys()
            .copied()
            .min_by_key(|id| id.raw())
            .unwrap_or(BastydeWindowId::new(0))
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

    /// Mutably iterate over all managed windows.
    #[allow(dead_code)]
    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut ManagedWindow> {
        self.windows.values_mut()
    }

    /// Get the winit WindowId for a BastydeWindowId.
    pub fn winit_id_for_bastyde(&self, bastyde_id: BastydeWindowId) -> Option<winit::window::WindowId> {
        self.bastyde_to_winit.get(&bastyde_id).copied()
    }

    /// Get the platform title bar host for a window, if the window opted
    /// into custom chrome via `WindowConfig::custom_chrome(true)` and the
    /// platform supports it. Returns `None` for windows that use native
    /// decorations or run on a window system without custom chrome support
    /// (currently X11).
    pub fn title_bar_host(&self, bastyde_id: BastydeWindowId) -> Option<Rc<dyn PlatformTitleBarHost>> {
        let winit_id = self.bastyde_to_winit.get(&bastyde_id).copied()?;
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

    /// Drain pending modal requests from all windows.
    pub fn drain_pending_modal_requests(
        &mut self,
    ) -> Vec<(BastydeWindowId, Vec<bastyde_core::QueuedModalRequest>)> {
        let mut all_requests = Vec::new();
        for managed in self.windows.values_mut() {
            let requests = managed.tree.drain_pending_modal_requests();
            if !requests.is_empty() {
                all_requests.push((managed.bastyde_id, requests));
            }
        }
        all_requests
    }

    /// Drain native modal-window dismiss requests from all windows.
    pub fn drain_pending_modal_dismissals(&mut self) -> Vec<BastydeWindowId> {
        let mut windows_to_close = Vec::new();
        for managed in self.windows.values_mut() {
            if managed.tree.drain_pending_modal_dismissal() && managed.modal {
                windows_to_close.push(managed.bastyde_id);
            }
        }
        windows_to_close
    }

    /// Drain per-tree close-window requests raised by handlers via
    /// [`EventContext::close_window`](bastyde_core::widget::EventContext::close_window).
    /// Returns `true` when at least one window will be closed.
    pub fn drain_close_window_requests(&mut self) -> bool {
        let mut to_close: Vec<BastydeWindowId> = Vec::new();
        for managed in self.windows.values_mut() {
            if managed.tree.take_close_window_request() {
                to_close.push(managed.bastyde_id);
            }
        }
        let any = !to_close.is_empty();
        for id in to_close {
            self.queue_close(id);
        }
        any
    }

    /// Drain per-tree locale-switch requests raised by handlers via
    /// [`EventContext::set_locale`](bastyde_core::widget::EventContext::set_locale),
    /// parse each one to a `LanguageIdentifier`, and route it through
    /// [`WindowManager::set_locale`] so the `I18nManager` (active locale,
    /// version signal, RTL direction) and every tree stay in sync.
    /// Invalid or unsupported locale strings are logged and dropped.
    pub fn drain_pending_locale_requests(&mut self) {
        let mut requests: Vec<String> = Vec::new();
        for managed in self.windows.values_mut() {
            if let Some(loc) = managed.tree.take_pending_locale_request() {
                requests.push(loc);
            }
        }
        for loc_str in requests {
            match loc_str.parse::<bastyde_i18n::LanguageIdentifier>() {
                Ok(loc) => self.set_locale(loc),
                Err(e) => {
                    eprintln!("bastyde-app: invalid locale `{loc_str}` requested by handler: {e}")
                }
            }
        }
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
        WindowCommand::Focus => win.focus_window(),
        WindowCommand::Close => {
            // Handled in drain_window_commands; unreachable here.
        }
    }
}

/// App-level implementation of [`bastyde_core::WindowOps`] handed into
/// every `dispatch_event_with_ops` call.
///
/// Holds `&mut WindowManager` plus `&ActiveEventLoop` so
/// [`open_window`](bastyde_core::WindowOps::open_window) can create the
/// winit-level window synchronously before returning. Constructed by
/// [`BastydeAppHandler::dispatch_in_window`] after temporarily removing
/// the dispatching window from `WindowManager::windows`; the removed
/// tree is borrowed mutably for the handler run.
pub struct WindowOpsImpl<'a> {
    wm: &'a mut WindowManager,
    event_loop: &'a winit::event_loop::ActiveEventLoop,
    /// Current (dispatching) window's id. Kept for diagnostics and
    /// future modal-parent self-reference logic.
    current_id: BastydeWindowId,
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
        current_id: BastydeWindowId,
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

impl bastyde_core::WindowOps for WindowOpsImpl<'_> {
    fn open_window(&mut self, config: bastyde_core::WindowConfig) -> BastydeWindowId {
        let _ = self.current_id;
        #[cfg(not(target_os = "macos"))]
        let _ = self.current_handle;
        #[cfg(target_os = "macos")]
        let _ = &self.current_window_arc;
        self.wm.create_window(config, self.event_loop)
    }

    fn find_window(&self, string_id: &str) -> Option<BastydeWindowId> {
        self.wm.find_window(string_id)
    }

    fn window_state(&self, id: BastydeWindowId) -> Option<bastyde_core::WindowState> {
        let winit_id = self.wm.bastyde_to_winit_map().get(&id).copied()?;
        self.wm
            .windows_map()
            .get(&winit_id)
            .map(|m| m.state.clone())
    }

    fn windows(&self) -> Vec<bastyde_core::WindowState> {
        self.wm
            .windows_map()
            .values()
            .map(|m| m.state.clone())
            .collect()
    }

    fn focus_window(&mut self, id: BastydeWindowId) {
        if let Some(winit_id) = self.wm.bastyde_to_winit_map().get(&id).copied()
            && let Some(managed) = self.wm.windows_map().get(&winit_id)
        {
            managed.platform_window.window().focus_window();
        }
    }

    fn close_window_by_id(&mut self, id: BastydeWindowId) {
        self.wm.queue_close(id);
    }

    fn current_parent_handle(&self) -> Option<bastyde_core::raw_handle::ParentHandle> {
        // Always extract from `current_window_arc` because the
        // dispatching window is temporarily out of `wm.windows_map()`
        // during event delivery.
        let arc = self.current_window_arc.as_ref()?;
        bastyde_core::raw_handle::ParentHandle::from_window(arc.as_ref())
    }
}
