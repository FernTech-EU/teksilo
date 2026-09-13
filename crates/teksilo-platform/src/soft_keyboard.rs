// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Raising and dismissing the platform's on-screen keyboard.
//!
//! A finger landing in a text field is the one input that has no desktop
//! answer: there is no physical keyboard behind it, and nothing in the
//! ordinary focus path summons a soft one. What each platform will do about
//! that differs so much that the honest surface is a *capability* — see
//! [`SoftKeyboardSupport`] — and three of the four desktop answers are "not
//! much".
//!
//! # Per platform
//!
//! - **Windows** — [`SoftKeyboardSupport::Explicit`]. The touch keyboard is
//!   `TabTip.exe`, driven through the undocumented `ITipInvocation` COM
//!   interface on the `UIHostNoLaunch` coclass. Its one method is `Toggle`:
//!   there is no *Show* and no *Hide*, so honouring both directions means
//!   reading the keyboard window's visibility first and toggling only when the
//!   state to change is the wrong one. [`should_toggle`] is that decision, and
//!   it is a pure function so it can be checked from any host.
//! - **macOS** — [`SoftKeyboardSupport::None`]. There is no client-facing
//!   request. The Accessibility Keyboard is a user setting under System
//!   Settings ▸ Accessibility ▸ Keyboard; `NSTextInputClient` raises the IME
//!   *candidate* window, which is not a keyboard.
//! - **Wayland** — [`SoftKeyboardSupport::None`], and this is the answer worth
//!   spelling out because it is not the one you would guess. Version 1 of
//!   `zwp_text_input_v3` — the version winit 0.30 binds, `1..=1` — has no
//!   `show_input_panel` request, so there is no explicit verb to send from
//!   where Teksilo stands. (Version **2** of the interface has since added
//!   `show_input_panel` / `hide_input_panel` back; reaching them means binding
//!   our own manager at that version, and whether that is worth doing turns on
//!   compositor support for a new interface version — see
//!   `docs/soft-keyboard.md`.) What a panel does instead is follow the
//!   `enable` + `commit` pair the framework already issues when a text widget
//!   takes focus — which is what [`SoftKeyboardSupport::ViaAccessibility`]
//!   describes, and Wayland still does not get that row, because that row is a
//!   *guarantee* and this is not one: mutter needs the pair **twice** before it
//!   shows the panel (GNOME/mutter issue #1506) and winit 0.30 sends it exactly
//!   once per `set_ime_allowed(true)`, so a GNOME session can end up with a
//!   focused field and no keyboard. Teksilo does not work around it, for a
//!   reason that is not laziness: the only reachable second `enable` is a
//!   second `set_ime_allowed(true)`, and `enable` is specified to reset "the
//!   state associated with preedit_string, commit_string, and
//!   delete_surrounding_text events" — it destroys a live composition. Binding
//!   a second `zwp_text_input_v3` of our own does not help either; the protocol
//!   says requests to enable a text input while another is enabled on the same
//!   seat must be ignored. So the choice is between a keyboard that sometimes
//!   does not appear and a composition that sometimes vanishes mid-word, and
//!   this is the side of it that loses no user data. Reporting `None` is the
//!   matching honesty: a widget that offers its own affordance is right on the
//!   session where nothing rises, and merely redundant on the session where
//!   something does.
//! - **X11** — [`SoftKeyboardSupport::None`]. On-screen keyboards are separate
//!   clients driven by AT-SPI or by the user; the core protocol, XInput2 and
//!   EWMH between them have no client request.
//!
//! # What `None` obliges a caller to do
//!
//! It is a contract, not a gap: where the answer is `None` the framework will
//! never raise a keyboard and promises nothing about whether the platform will,
//! so a text surface that expects to be driven by a finger has to offer its own
//! affordance. Read it through
//! [`EventContext::soft_keyboard_support`](teksilo_core::widget::EventContext::soft_keyboard_support).

use teksilo_core::window::SoftKeyboardSupport;

use crate::pointer_backend::PlatformKind;

/// What `platform` can do about an on-screen keyboard.
///
/// A pure function of the platform, like
/// [`BackendCaps::for_platform`](crate::pointer_backend::BackendCaps::for_platform),
/// so every row can be asserted from any host.
pub const fn support_for(platform: PlatformKind) -> SoftKeyboardSupport {
    match platform {
        PlatformKind::Windows => SoftKeyboardSupport::Explicit,
        PlatformKind::MacOs | PlatformKind::Unix => SoftKeyboardSupport::None,
    }
}

/// What the host this binary was compiled for can do.
pub const fn support() -> SoftKeyboardSupport {
    support_for(PlatformKind::HOST)
}

/// What the framework should do about a pending soft-keyboard request.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SoftKeyboardAction {
    /// Nothing — either the framework has no keyboard request to send, or the
    /// request is already satisfied by the IME-allowance reconcile.
    Nothing,
    /// Ask the platform to show (`true`) or hide (`false`) the keyboard.
    Ask(bool),
}

/// Resolve a pending request against what the platform can do.
///
/// The rule that matters is the [`SoftKeyboardSupport::ViaAccessibility`] one:
/// *asking* there means re-asserting IME allowance, and re-asserting allowance
/// is what destroys a live composition — so on that platform the framework's
/// ordinary focus-driven reconcile is the only request that will ever be made,
/// and an explicit one resolves to nothing. That is what makes placing a caret
/// with a finger mid-composition safe: the request cannot reach
/// `set_ime_allowed` because nothing on this path calls it.
///
/// [`SoftKeyboardSupport::Explicit`] has no such hazard — its request goes to
/// the keyboard's own control, not through the IME channel — so it is passed
/// through, and the *state* question ("is it already up?") belongs to
/// [`should_toggle`] inside the platform call.
pub const fn resolve(support: SoftKeyboardSupport, want_visible: bool) -> SoftKeyboardAction {
    match support {
        SoftKeyboardSupport::None | SoftKeyboardSupport::ViaAccessibility => {
            SoftKeyboardAction::Nothing
        }
        SoftKeyboardSupport::Explicit => SoftKeyboardAction::Ask(want_visible),
        // `SoftKeyboardSupport` is `#[non_exhaustive]`: a variant added later
        // has not been thought about here, and doing nothing is the answer
        // that cannot cancel a composition.
        _ => SoftKeyboardAction::Nothing,
    }
}

/// Whether a toggle-only keyboard control has to be poked.
///
/// The whole of the Windows decision, extracted because `ITipInvocation` offers
/// only `Toggle`: poking it when the keyboard is already in the wanted state
/// puts it in the wrong one, which is how a naive "always show" ends up hiding
/// the keyboard it was asked to raise.
pub const fn should_toggle(currently_visible: bool, want_visible: bool) -> bool {
    currently_visible != want_visible
}

/// Ask the platform to show or hide its on-screen keyboard.
///
/// Returns `true` when the platform both understood the request and acted on
/// it — which is `false` on every platform reporting
/// [`SoftKeyboardSupport::None`], and `false` on Windows when the keyboard is
/// already in the wanted state (nothing to do is not a failure to the caller,
/// but it is also not an action, and the app layer uses the distinction only
/// for tracing).
pub fn set_visible(window: &winit::window::Window, visible: bool) -> bool {
    #[cfg(target_os = "windows")]
    {
        windows_impl::set_visible(window, visible)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, visible);
        false
    }
}

/// The rectangle the on-screen keyboard currently covers, in **screen**
/// coordinates and physical pixels, or `None` when no keyboard is up.
///
/// Windows only, and for the same reason as [`set_visible`]: it is the only
/// desktop platform whose keyboard is a findable window. macOS has no keyboard
/// to find; `zwp_text_input_v3` has no event carrying the input panel's
/// geometry (enter, leave, preedit_string, commit_string,
/// delete_surrounding_text, done and the newer action/language/preedit_hint —
/// none of them a rectangle); and under X11 the keyboard is an unrelated
/// client with no hint saying where it is.
pub fn keyboard_screen_rect() -> Option<(i32, i32, i32, i32)> {
    #[cfg(target_os = "windows")]
    {
        windows_impl::keyboard_screen_rect()
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

#[cfg(target_os = "windows")]
mod windows_impl {
    //! **Verification status:** written against the `windows` crate 0.62 API
    //! and the published `ITipInvocation` GUIDs. It is
    //! `cfg(target_os = "windows")` and has **not** been exercised on a
    //! Windows host — the touch-keyboard model changed in Windows 11 22H2, so
    //! this is a P44 hardware sign-off item. Everything decidable without an
    //! OS ([`super::should_toggle`], the capability row) is unit-tested.

    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_HANDLER, CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance,
        CoInitializeEx,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetDesktopWindow, GetWindowRect, IsWindowVisible,
    };
    use windows::core::{GUID, HRESULT, IUnknown, interface, w};

    /// `UIHostNoLaunch` — the coclass that talks to an *already running*
    /// `TabTip.exe` without starting one.
    const CLSID_UI_HOST_NO_LAUNCH: GUID = GUID::from_u128(0x4ce576fa_83dc_4f88_951c_9d0782b4e376);

    /// The touch keyboard's own top-level window class.
    ///
    /// Two of them exist across Windows versions: `IPTip_Main_Window` is the
    /// classic one, and Windows 10+ hosts the keyboard inside an
    /// `ApplicationFrameWindow` whose title is the keyboard's. Both are
    /// checked, because which one answers depends on the OS build.
    fn keyboard_window() -> Option<HWND> {
        unsafe {
            if let Ok(hwnd) = FindWindowW(w!("IPTip_Main_Window"), None)
                && !hwnd.is_invalid()
            {
                return Some(hwnd);
            }
            if let Ok(hwnd) = FindWindowW(
                w!("ApplicationFrameWindow"),
                w!("Microsoft Text Input Application"),
            ) && !hwnd.is_invalid()
            {
                return Some(hwnd);
            }
        }
        None
    }

    fn is_visible() -> bool {
        keyboard_window().is_some_and(|hwnd| unsafe { IsWindowVisible(hwnd) }.as_bool())
    }

    #[interface("37c994e7-432b-4834-a2f7-dce1f13b834b")]
    unsafe trait ITipInvocation: IUnknown {
        fn Toggle(&self, hwnd: HWND) -> HRESULT;
    }

    pub(super) fn set_visible(_window: &winit::window::Window, visible: bool) -> bool {
        if !super::should_toggle(is_visible(), visible) {
            return false;
        }
        unsafe {
            // The winit main thread is already an STA (winit's own OLE
            // initialisation for drag-and-drop), so this is normally a
            // no-op returning `S_FALSE`; calling it makes the module
            // correct on a thread that is not.
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let Ok(tip) = CoCreateInstance::<_, ITipInvocation>(
                &CLSID_UI_HOST_NO_LAUNCH,
                None,
                CLSCTX_INPROC_HANDLER | CLSCTX_LOCAL_SERVER,
            ) else {
                return false;
            };
            // `Toggle` takes the window the keyboard should position itself
            // against; the documented value is the desktop window, which is
            // what every known consumer passes.
            tip.Toggle(GetDesktopWindow()).is_ok()
        }
    }

    pub(super) fn keyboard_screen_rect() -> Option<(i32, i32, i32, i32)> {
        let hwnd = keyboard_window()?;
        if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
            return None;
        }
        let mut rect = windows::Win32::Foundation::RECT::default();
        unsafe { GetWindowRect(hwnd, &mut rect) }.ok()?;
        Some((rect.left, rect.top, rect.right, rect.bottom))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_platform_row_is_stated() {
        // Windows is the only desktop platform with a request to make, and
        // Unix covers both Wayland and X11 because neither has one: Wayland's
        // v3 text-input interface winit binds has no `show_input_panel`, and
        // X11 never had a client request at all. Wayland is not
        // `ViaAccessibility` either: that row promises a panel *will* rise on
        // the IME enable, and mutter's double-`enable` requirement means it
        // sometimes does not.
        assert_eq!(
            support_for(PlatformKind::Windows),
            SoftKeyboardSupport::Explicit
        );
        assert_eq!(support_for(PlatformKind::MacOs), SoftKeyboardSupport::None);
        assert_eq!(support_for(PlatformKind::Unix), SoftKeyboardSupport::None);
    }

    #[test]
    fn a_request_never_reaches_the_ime_channel() {
        // The one rule that protects a live composition: where "ask" would
        // mean "re-assert IME allowance", the framework does not ask.
        assert_eq!(
            resolve(SoftKeyboardSupport::ViaAccessibility, true),
            SoftKeyboardAction::Nothing
        );
        assert_eq!(
            resolve(SoftKeyboardSupport::ViaAccessibility, false),
            SoftKeyboardAction::Nothing
        );
        // Nothing to raise at all.
        assert_eq!(
            resolve(SoftKeyboardSupport::None, true),
            SoftKeyboardAction::Nothing
        );
        // A keyboard with its own control is asked in both directions; it is
        // `should_toggle`, not this, that knows whether the ask is a no-op.
        assert_eq!(
            resolve(SoftKeyboardSupport::Explicit, true),
            SoftKeyboardAction::Ask(true)
        );
        assert_eq!(
            resolve(SoftKeyboardSupport::Explicit, false),
            SoftKeyboardAction::Ask(false)
        );
    }

    #[test]
    fn a_toggle_only_control_is_poked_only_when_the_state_is_wrong() {
        // The whole reason `Explicit` is honest on Windows: `Toggle` would
        // *hide* a keyboard that is already up if it were called blind.
        assert!(should_toggle(false, true), "hidden, asked to show");
        assert!(should_toggle(true, false), "shown, asked to hide");
        assert!(!should_toggle(true, true), "already shown");
        assert!(!should_toggle(false, false), "already hidden");
    }
}
