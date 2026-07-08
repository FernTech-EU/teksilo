// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Wayland `xdg_activation_v1` raise of an existing window.
//!
//! Placeholder until the KWin feasibility spike (plan A2) confirms that
//! activating an existing surface with a peer-minted token actually raises the
//! window. Until then, activation degrades to an attention request — the same
//! behaviour as calling `raise` with no token.

use winit::window::Window;

/// Raise `window` using an `xdg_activation_v1` `token` minted by the requester.
///
/// TODO(plan A2): bind `xdg_activation_v1` over winit's shared `wl_display` (see
/// `external_dnd::wayland` for the connection / surface-reconstruction pattern)
/// and call `activate(token, wl_surface)`. For now, degrade to attention.
pub(super) fn activate_with_token(window: &Window, _token: &str) {
    super::request_attention(window);
}
