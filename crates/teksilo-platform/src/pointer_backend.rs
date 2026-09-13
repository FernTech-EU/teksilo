// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The seam between an OS input backend and Teksilo's pointer vocabulary.
//!
//! Everything above this module speaks [`PointerSample`] / [`ScrollSample`] /
//! [`GestureEvent`]. Everything below it speaks whatever the window system
//! speaks. [`PointerBackend`] is the one door between the two, and
//! [`BackendCaps`] is the honest declaration of what the backend on the other
//! side can and cannot report.
//!
//! # Why a trait rather than a function
//!
//! Three reasons, in the order they will bite:
//!
//! 1. **The winit 0.31 upgrade.** winit 0.30 models touch as
//!    `WindowEvent::Touch { id: u64, phase: TouchPhase, force, .. }`; 0.31
//!    replaces it with a unified pointer API (`PointerKind`, `PointerSource`,
//!    `FingerId`, the `TabletTool*` family). That is a rewrite of one
//!    implementation of this trait, not of the framework.
//! 2. **Testing without an OS.** The conformance suite
//!    (`tests/backend_conformance.rs`) drives *recorded* event vectors through
//!    this trait and checks six invariants. A future backend earns its trust
//!    by passing the same suite.
//! 3. **Honesty about capability.** A consumer that must know whether cancels
//!    are reported — or whether a finger can drag the window — should ask,
//!    not guess from `cfg!(target_os = ...)`. [`BackendCaps`] is that answer,
//!    and every `false` in it is a documented platform fact rather than a
//!    to-do.
//!
//! Reference: `docs/touch-and-pen.md`, "Platform capabilities".

use teksilo_core::gesture::GestureEvent;
use teksilo_core::pointer::{EventTime, PointerSample, ScrollSample};

use crate::window_system::WindowSystem;

// ---------------------------------------------------------------------------
// The samples a backend produces
// ---------------------------------------------------------------------------

/// One translated input sample, ready to enter a widget tree.
///
/// A single OS packet can produce zero, one or several of these — a winit
/// `CursorMoved` that the X11 phantom-motion filter drops produces none, a
/// [`PointerBackend::cancel_all`] with three fingers down produces three.
#[derive(Clone, Debug)]
pub enum InputSample {
    /// Route through
    /// [`WidgetTree::dispatch_pointer`](teksilo_core::WidgetTree::dispatch_pointer).
    Pointer(PointerSample),
    /// Route through
    /// [`WidgetTree::dispatch_scroll`](teksilo_core::WidgetTree::dispatch_scroll).
    Scroll(ScrollSample),
    /// An already-recognised gesture handed over by the OS — a trackpad pinch,
    /// rotation or smart-magnification double tap. Teksilo does not
    /// re-recognise these: the OS driver has better data (raw touch on the
    /// trackpad surface) than the framework ever will.
    Gesture(GestureEvent),
}

impl InputSample {
    /// The pointer sample, if this is one. Convenience for the conformance
    /// harness and for a caller that only cares about one arm.
    pub fn as_pointer(&self) -> Option<&PointerSample> {
        match self {
            Self::Pointer(sample) => Some(sample),
            _ => None,
        }
    }

    /// The scroll sample, if this is one.
    pub fn as_scroll(&self) -> Option<&ScrollSample> {
        match self {
            Self::Scroll(sample) => Some(sample),
            _ => None,
        }
    }

    /// The gesture, if this is one.
    pub fn as_gesture(&self) -> Option<&GestureEvent> {
        match self {
            Self::Gesture(gesture) => Some(gesture),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// The events a backend consumes
// ---------------------------------------------------------------------------

/// One OS packet on its way into [`PointerBackend::translate`].
///
/// This crate already re-exports winit types across its whole event-translation
/// surface (`translate_mouse_button`, `translate_ime`, `translate_key` all take
/// them), so borrowing a `winit::event::WindowEvent` here leaks nothing new. It
/// is an enum rather than a bare reference so that a backend for a different
/// window system — or a replay harness reading a recorded trace — can be added
/// as a variant without breaking the trait.
///
/// `#[non_exhaustive]`: adding a variant must not be a breaking change.
#[non_exhaustive]
#[derive(Debug)]
pub enum BackendEvent<'a> {
    /// A winit window event, borrowed from the event loop.
    Winit(&'a winit::event::WindowEvent),
}

impl<'a> From<&'a winit::event::WindowEvent> for BackendEvent<'a> {
    fn from(event: &'a winit::event::WindowEvent) -> Self {
        Self::Winit(event)
    }
}

// ---------------------------------------------------------------------------
// Capabilities
// ---------------------------------------------------------------------------

/// How a platform can be asked to raise the on-screen keyboard.
///
/// Re-exported from `teksilo-core`, where it lives so that
/// [`WindowOps::soft_keyboard_support`](teksilo_core::window::WindowOps::soft_keyboard_support)
/// can carry it to a widget without the widget layer depending on this crate.
/// The per-platform values, and the reason behind each, are in
/// [`crate::soft_keyboard`].
pub use teksilo_core::window::SoftKeyboardSupport;

/// Which OS a [`BackendCaps`] row describes.
///
/// A plain enum rather than `cfg!` so the capability matrix is a *pure
/// function* and every row of it can be asserted from any host — a Linux CI
/// runner tests the macOS and Windows rows too. [`Self::HOST`] is the row for
/// the machine this binary was compiled for.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum PlatformKind {
    /// Windows 8 or later.
    Windows,
    /// macOS.
    MacOs,
    /// Linux, the BSDs, and anything else winit drives through its
    /// Wayland/X11 backends.
    Unix,
}

impl PlatformKind {
    /// The platform this binary targets.
    pub const HOST: Self = {
        #[cfg(target_os = "windows")]
        {
            Self::Windows
        }
        #[cfg(target_os = "macos")]
        {
            Self::MacOs
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            Self::Unix
        }
    };
}

/// What a backend can actually report.
///
/// Every field is a *platform fact* established by reading the backend, not an
/// aspiration. The per-OS matrix and its citations are in
/// `docs/touch-and-pen.md`, "Platform capabilities"; the machine-readable
/// version is [`BackendCaps::for_platform`].
///
/// `#[non_exhaustive]`: construct one with
/// [`for_platform`](Self::for_platform) and adjust, so a later capability
/// cannot break a call site.
#[non_exhaustive]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct BackendCaps {
    /// The backend reports a *cancel* distinct from a lift. When `false`, a
    /// revoked contact arrives as an ordinary `Ended` (or as nothing at all)
    /// and the framework has to infer revocation from focus loss.
    pub reports_cancel: bool,
    /// Contacts carry a tip pressure.
    pub reports_pressure: bool,
    /// Stylus tilt is reported.
    pub reports_tilt: bool,
    /// Stylus barrel rotation is reported.
    pub reports_twist: bool,
    /// The backend distinguishes a pen tip from an eraser.
    pub reports_pen_kind: bool,
    /// The backend flags a contact the digitizer classified as a palm.
    pub reports_palm: bool,
    /// Scroll samples carry a begin/change/end structure rather than being
    /// bare notches.
    pub reports_scroll_phase: bool,
    /// The OS produces the momentum (inertial) part of a scroll itself, so the
    /// framework must **not** add a fling of its own on top.
    pub reports_os_momentum: bool,
    /// The OS recognises pinch/rotate on the trackpad and hands over the
    /// result.
    pub reports_os_pinch: bool,
    /// The OS also synthesises a mouse stream from touch, so the translator
    /// must suppress one of the two.
    pub synthesises_mouse_from_touch: bool,
    /// A finger can start an OS window move/resize.
    pub touch_window_drag: bool,
    /// How the on-screen keyboard can be reached.
    pub osk: SoftKeyboardSupport,
}

impl BackendCaps {
    /// The capability row for one platform, and — on Unix — one window system.
    ///
    /// `window_system` is ignored off Unix: `WindowSystem` only ever resolves
    /// to `Wayland` or `X11` from a live Linux/BSD display handle, and
    /// [`WindowSystem::Unknown`] is precisely the set {Windows, macOS,
    /// headless}. That is what makes `Unknown` a safe default for the
    /// mouse-promotion suppressors — none of those three platforms promote.
    ///
    /// Every value here is cited in `docs/touch-and-pen.md`.
    pub const fn for_platform(platform: PlatformKind, window_system: WindowSystem) -> Self {
        match platform {
            // winit registers `RegisterTouchWindow(hwnd, TWF_WANTPALM)` and
            // handles `WM_TOUCH` *and* the `WM_POINTER*` family, returning 0
            // without calling `DefWindowProc` — so Windows never promotes a
            // finger to a mouse click, and there is no dual stream to fight.
            //
            // `WM_TOUCH` reports `force: None`; the `WM_POINTER*` path
            // normalises `POINTER_TOUCH_INFO::pressure` / `POINTER_PEN_INFO::
            // pressure` over 1..=1024, so pressure is reachable. Tilt, twist,
            // eraser and the palm flag all exist in `POINTER_PEN_INFO` /
            // `TOUCH_FLAG_PALM` but winit 0.30 does not surface them.
            PlatformKind::Windows => Self {
                reports_cancel: false,
                reports_pressure: true,
                reports_tilt: false,
                reports_twist: false,
                reports_pen_kind: false,
                reports_palm: false,
                // `MouseWheel` always arrives with `TouchPhase::Moved`;
                // precision-touchpad phase information is not surfaced.
                reports_scroll_phase: false,
                reports_os_momentum: false,
                reports_os_pinch: false,
                synthesises_mouse_from_touch: false,
                // `WM_NCLBUTTONDOWN`-based drag is a mouse path; winit's
                // `drag_window` sends it and a finger does not reach it.
                touch_window_drag: false,
                osk: crate::soft_keyboard::support_for(PlatformKind::Windows),
            },
            // macOS delivers **no touch at all**: `WindowEvent::Touch` is
            // documented "macOS: Unsupported". The trackpad arrives as
            // `PinchGesture` / `RotationGesture` / `DoubleTapGesture`, and a
            // two-finger pan as `MouseWheel` pixel deltas whose phase winit
            // folds momentum into (see `TranslationState`'s scroll machine).
            PlatformKind::MacOs => Self {
                reports_cancel: false,
                reports_pressure: false,
                reports_tilt: false,
                reports_twist: false,
                reports_pen_kind: false,
                reports_palm: false,
                reports_scroll_phase: true,
                reports_os_momentum: true,
                reports_os_pinch: true,
                synthesises_mouse_from_touch: false,
                touch_window_drag: false,
                osk: crate::soft_keyboard::support_for(PlatformKind::MacOs),
            },
            PlatformKind::Unix => match window_system {
                // `wl_touch.cancel` is the only desktop source of a real
                // cancel in winit 0.30. Wayland's `wl_touch.down/motion/up`
                // carry no pressure and no tool axes.
                //
                // `touch_window_drag` is **false**, and this one is worth
                // stating plainly: `xdg_toplevel::move` needs a serial from an
                // input event on a toplevel the compositor agrees the client
                // owns, and winit 0.30's `drag_window` harvests a *pointer*
                // serial internally. A finger therefore cannot start a window
                // move under winit 0.30, however the app asks.
                WindowSystem::Wayland => Self {
                    reports_cancel: true,
                    reports_pressure: false,
                    reports_tilt: false,
                    reports_twist: false,
                    reports_pen_kind: false,
                    reports_palm: false,
                    reports_scroll_phase: true,
                    reports_os_momentum: false,
                    reports_os_pinch: false,
                    synthesises_mouse_from_touch: false,
                    touch_window_drag: false,
                    osk: crate::soft_keyboard::support_for(PlatformKind::Unix),
                },
                // XI2 touch. winit filters *emulated button* events
                // (`XIPointerEmulated`) but synthesises a `CursorMoved` of its
                // own for the first concurrently-active contact, on every
                // phase of it — so the emulated motion stream is real and must
                // be suppressed here.
                //
                // No `XI_TouchCancel` handling: winit 0.30 never emits
                // `TouchPhase::Cancelled` on X11. `force: None // TODO`.
                WindowSystem::X11 => Self {
                    reports_cancel: false,
                    reports_pressure: false,
                    reports_tilt: false,
                    reports_twist: false,
                    reports_pen_kind: false,
                    reports_palm: false,
                    reports_scroll_phase: false,
                    reports_os_momentum: false,
                    reports_os_pinch: false,
                    synthesises_mouse_from_touch: true,
                    touch_window_drag: false,
                    osk: crate::soft_keyboard::support_for(PlatformKind::Unix),
                },
                // A headless or not-yet-created window. Report nothing: an
                // unknown backend must not be credited with a capability.
                WindowSystem::Unknown => Self {
                    reports_cancel: false,
                    reports_pressure: false,
                    reports_tilt: false,
                    reports_twist: false,
                    reports_pen_kind: false,
                    reports_palm: false,
                    reports_scroll_phase: false,
                    reports_os_momentum: false,
                    reports_os_pinch: false,
                    synthesises_mouse_from_touch: false,
                    touch_window_drag: false,
                    osk: crate::soft_keyboard::support_for(PlatformKind::Unix),
                },
            },
        }
    }
}

// ---------------------------------------------------------------------------
// The trait
// ---------------------------------------------------------------------------

/// Turns OS packets into [`InputSample`]s.
///
/// One instance per window: a backend owns per-window state (the live contact
/// set, the scroll-phase machine, the modifier and scale factor) and two
/// windows must not share it.
///
/// # Contract
///
/// - Every contact that produces a [`PointerPhase::Down`] sample is terminated
///   by exactly one [`Up`](teksilo_core::PointerPhase::Up) or one
///   [`Cancel`](teksilo_core::PointerPhase::Cancel) — never both, never
///   neither. [`cancel_all`](Self::cancel_all) exists so a caller can honour
///   that when the window goes away.
/// - The [`EventTime`]s a backend stamps are monotone non-decreasing within
///   one stream. `now` is supplied by the caller from the tree's one clock, so
///   a backend never reads `Instant::now()`.
/// - A backend never allocates a [`PointerId`](teksilo_core::PointerId)
///   itself: it mints through
///   [`PointerIdAllocator`](teksilo_core::PointerIdAllocator), which is what
///   makes a reused OS contact id resolve to a fresh identity.
///
/// [`PointerPhase::Down`]: teksilo_core::PointerPhase::Down
pub trait PointerBackend {
    /// Translate one OS packet. Returns every sample it implies, in dispatch
    /// order; an empty vector is a normal, common answer (a filtered phantom,
    /// a touch packet with the kill switch off, an unmapped mouse button).
    fn translate(&mut self, event: &BackendEvent<'_>, now: EventTime) -> Vec<InputSample>;

    /// What this backend can report. See [`BackendCaps`].
    fn capabilities(&self) -> BackendCaps;

    /// Terminate every live contact with a
    /// [`Cancel`](teksilo_core::PointerPhase::Cancel) sample and forget it.
    ///
    /// Called when the window loses the input it was receiving — focus loss, a
    /// close, a compositor grab — so that the "every Down is terminated"
    /// contract survives a stream that simply stops.
    fn cancel_all(&mut self, now: EventTime) -> Vec<InputSample>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one capability the programme most wants to be wrong about. It is
    /// not: winit 0.30's `drag_window` harvests a pointer serial, so no
    /// platform can start a window drag from a finger.
    #[test]
    fn no_platform_offers_touch_window_drag() {
        for platform in [PlatformKind::Windows, PlatformKind::MacOs] {
            assert!(!BackendCaps::for_platform(platform, WindowSystem::Unknown).touch_window_drag);
        }
        for ws in [
            WindowSystem::Wayland,
            WindowSystem::X11,
            WindowSystem::Unknown,
        ] {
            assert!(!BackendCaps::for_platform(PlatformKind::Unix, ws).touch_window_drag);
        }
    }

    /// Wayland is the only desktop backend that reports a real cancel.
    #[test]
    fn only_wayland_reports_cancel() {
        assert!(
            BackendCaps::for_platform(PlatformKind::Unix, WindowSystem::Wayland).reports_cancel
        );
        assert!(!BackendCaps::for_platform(PlatformKind::Unix, WindowSystem::X11).reports_cancel);
        assert!(
            !BackendCaps::for_platform(PlatformKind::Windows, WindowSystem::Unknown).reports_cancel
        );
        assert!(
            !BackendCaps::for_platform(PlatformKind::MacOs, WindowSystem::Unknown).reports_cancel
        );
    }

    /// X11 is the only backend whose emulated pointer has to be suppressed.
    #[test]
    fn only_x11_promotes_touch_to_mouse() {
        assert!(
            BackendCaps::for_platform(PlatformKind::Unix, WindowSystem::X11)
                .synthesises_mouse_from_touch
        );
        for (platform, ws) in [
            (PlatformKind::Unix, WindowSystem::Wayland),
            (PlatformKind::Unix, WindowSystem::Unknown),
            (PlatformKind::Windows, WindowSystem::Unknown),
            (PlatformKind::MacOs, WindowSystem::Unknown),
        ] {
            assert!(!BackendCaps::for_platform(platform, ws).synthesises_mouse_from_touch);
        }
    }

    /// macOS owns the momentum, so P12 must never add a fling there.
    #[test]
    fn only_macos_owns_the_momentum() {
        assert!(
            BackendCaps::for_platform(PlatformKind::MacOs, WindowSystem::Unknown)
                .reports_os_momentum
        );
        assert!(
            !BackendCaps::for_platform(PlatformKind::Unix, WindowSystem::Wayland)
                .reports_os_momentum
        );
        assert!(
            !BackendCaps::for_platform(PlatformKind::Windows, WindowSystem::Unknown)
                .reports_os_momentum
        );
    }

    /// Windows is the only desktop backend that reaches a soft keyboard.
    ///
    /// It was `ViaAccessibility` while nothing could ask for the keyboard: the
    /// Windows touch keyboard does rise for a UIA text pattern under touch
    /// focus, and that was the whole of what the framework could claim. It is
    /// [`SoftKeyboardSupport::Explicit`] now that
    /// [`crate::soft_keyboard::set_visible`] exists and honours **both**
    /// directions — which is the bar `Explicit` sets, and why a `Toggle`-only
    /// COM call needs the visibility probe beside it.
    #[test]
    fn the_soft_keyboard_is_windows_only_and_explicit() {
        assert_eq!(
            BackendCaps::for_platform(PlatformKind::Windows, WindowSystem::Unknown).osk,
            SoftKeyboardSupport::Explicit
        );
        assert_eq!(
            BackendCaps::for_platform(PlatformKind::Unix, WindowSystem::Wayland).osk,
            SoftKeyboardSupport::None
        );
        assert_eq!(SoftKeyboardSupport::default(), SoftKeyboardSupport::None);
    }

    /// An unknown window system is credited with nothing.
    #[test]
    fn an_unknown_backend_claims_nothing() {
        assert_eq!(
            BackendCaps::for_platform(PlatformKind::Unix, WindowSystem::Unknown),
            BackendCaps::default()
        );
    }
}
