// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Cross-platform window "raise / activate" — the per-OS backend behind
//! `WindowState::focus()` / `WindowOps::focus_window`.
//!
//! winit's `Window::focus_window` already performs a real cross-process raise on
//! X11 (`_NET_ACTIVE_WINDOW`), Windows (`SetForegroundWindow` + the SendInput
//! foreground-lock defeat) and macOS (`activateIgnoringOtherApps` +
//! `makeKeyAndOrderFront`) — including when the *target* window raises itself in
//! response to a cross-process request. On **Wayland it is a hard no-op**, so a
//! real raise there requires driving `xdg_activation_v1` over the window's raw
//! `wl_surface` (see the [`wayland`] submodule).
//!
//! Callers keep the single `focus()` API. The optional activation `token` — an
//! opaque string minted by the focused requester and handed across the process
//! boundary — is only ever consulted on Wayland; everywhere else it is ignored
//! because `focus_window()` already suffices.

use winit::window::Window;

#[cfg(all(unix, not(target_os = "macos")))]
mod wayland;

/// Raise `window` above others and give it keyboard focus, best-effort.
///
/// `token` is only meaningful on Wayland (an `xdg_activation_v1` token). On
/// Wayland *without* a token a genuine focus-steal is impossible, so this
/// degrades to an attention request (urgency hint / taskbar highlight). On every
/// other platform the token is ignored and winit's `focus_window()` performs the
/// raise.
pub fn raise(window: &Window, token: Option<&str>) {
    #[cfg(all(unix, not(target_os = "macos")))]
    if crate::active_window_system() == crate::WindowSystem::Wayland {
        match token {
            Some(token) => wayland::activate_with_token(window, token),
            None => request_attention(window),
        }
        return;
    }

    let _ = token;
    window.focus_window();
}

/// Consume an `xdg_activation_v1` startup token from the environment (set by the
/// process that spawned us) and apply it to `attrs`, so the created window comes
/// up focused on Wayland. Resets the env afterwards so later windows / grandchild
/// processes don't reuse a stale token. No-op off Wayland/X11.
pub fn apply_creation_token(
    attrs: winit::window::WindowAttributes,
    event_loop: &winit::event_loop::ActiveEventLoop,
) -> winit::window::WindowAttributes {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use winit::platform::startup_notify::{
            EventLoopExtStartupNotify, WindowAttributesExtStartupNotify, reset_activation_token_env,
        };
        if let Some(token) = event_loop.read_token_from_env() {
            reset_activation_token_env();
            return attrs.with_activation_token(token);
        }
    }
    let _ = event_loop;
    attrs
}

/// Ask the compositor for an `xdg_activation_v1` token for `window` (to hand to
/// another window or a child process). Returns `true` if a request was issued —
/// the token then arrives via `WindowEvent::ActivationTokenDone`. Returns `false`
/// where unsupported (everything but Wayland/X11), so the caller treats "no
/// token" as immediate.
pub fn request_activation_token(window: &Window) -> bool {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        use winit::platform::startup_notify::WindowExtStartupNotify;
        window.request_activation_token().is_ok()
    }
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    {
        let _ = window;
        false
    }
}

/// Inject an `xdg_activation_v1` `token` into a child process's environment so its
/// first window (built with `WindowConfig::activate_from_env`) comes up focused on
/// Wayland. Sets both the Wayland and X11 startup-notify variables that winit's
/// `read_token_from_env` looks for. No-op off Wayland/X11.
pub fn set_child_activation_env(cmd: &mut std::process::Command, token: &str) {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        cmd.env("XDG_ACTIVATION_TOKEN", token);
        cmd.env("DESKTOP_STARTUP_ID", token);
    }
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    {
        let _ = (cmd, token);
    }
}

/// The Wayland degrade (no token, or until the Tier-2 raise lands): a one-shot
/// informational attention request. winit implements this on Wayland via
/// `xdg_activation_v1` against the window's own surface.
#[cfg(all(unix, not(target_os = "macos")))]
fn request_attention(window: &Window) {
    window.request_user_attention(Some(winit::window::UserAttentionType::Informational));
}
