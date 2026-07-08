// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Wayland `xdg_activation_v1` raise of an existing window.
//!
//! winit's `Window::focus_window()` is a no-op on Wayland, so raising an
//! already-open window means driving `xdg_activation_v1.activate(token, surface)`
//! ourselves. The `token` is minted by the *focused* requester (via
//! [`request_activation_token`](super::request_activation_token)) and handed
//! here; the compositor decides whether to honour it (a token from an unfocused
//! client is dropped for focus-stealing prevention — hence "requester must be
//! focused").
//!
//! Like [`crate::external_dnd::wayland`], we bind our own client objects on
//! **winit's own `wl_display`** ([`Backend::from_foreign_display`]) so the
//! surface object-ids match. `activate` is a one-shot, reply-less request, so we
//! only ever *write* (flush) — we never read the socket, which winit's event
//! loop owns as sole reader. On failure (X11, missing global, bad handle) we
//! degrade to an attention request.

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use wayland_backend::client::ObjectId;
use wayland_backend::sys::client::Backend;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::xdg::activation::v1::client::xdg_activation_v1::XdgActivationV1;
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::Window;

/// Raise `window` by handing `token` to the compositor via `xdg_activation_v1`.
/// Falls back to an attention request if the protocol path can't run.
pub(super) fn activate_with_token(window: &Window, token: &str) {
    if activate(window, token).is_none() {
        // X11, missing global, or a bad handle — fall back to urgency.
        super::request_attention(window);
    }
}

/// Dispatch sink — `xdg_activation_v1` and the registry emit no events we care
/// about, so every handler is empty. Required by `registry_queue_init`.
struct ActivationState;

impl Dispatch<WlRegistry, GlobalListContents> for ActivationState {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<XdgActivationV1, ()> for ActivationState {
    fn event(
        _: &mut Self,
        _: &XdgActivationV1,
        _: <XdgActivationV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

fn activate(window: &Window, token: &str) -> Option<()> {
    let RawDisplayHandle::Wayland(display) = window.display_handle().ok()?.as_raw() else {
        return None;
    };
    let RawWindowHandle::Wayland(surface_handle) = window.window_handle().ok()?.as_raw() else {
        return None;
    };

    // Share winit's connection so surface object-ids line up.
    let backend = unsafe { Backend::from_foreign_display(display.display.as_ptr() as *mut _) };
    let conn = Connection::from_backend(backend);

    // One-time registry roundtrip to find the global. Safe here because this
    // runs on the UI thread during a command drain (winit is not mid-read).
    let (globals, queue) = registry_queue_init::<ActivationState>(&conn).ok()?;
    let qh = queue.handle();
    let activation = globals.bind::<XdgActivationV1, _, _>(&qh, 1..=1, ()).ok()?;

    let surface_id = unsafe {
        ObjectId::from_ptr(
            WlSurface::interface(),
            surface_handle.surface.as_ptr() as *mut _,
        )
    }
    .ok()?;
    let surface = WlSurface::from_id(&conn, surface_id).ok()?;

    activation.activate(token.to_string(), &surface);
    // Write the request out. Send-only, so no read of the winit-owned socket.
    conn.flush().ok()?;
    Some(())
}
