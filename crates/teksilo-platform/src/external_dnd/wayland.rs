// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Wayland external drag-and-drop backend (`wl_data_device`).
//!
//! winit (0.30) does not implement Wayland drag-and-drop, so there is no
//! competing device of winit's to displace. The device itself is not this
//! module's: it is the process's one `wl_data_device`, bound on winit's own
//! `wl_display` by [`WaylandDataHub`], which the clipboard uses too. Mutter keeps
//! one device per client and silences the others, so a device per window, or a
//! device for drags beside one for the clipboard, would leave one of them
//! deaf; the hub's module documentation has the detail.
//!
//! Each window publishes its surface on the hub when it is attached and
//! withdraws it when its guard drops; the hub delivers a drag to the window
//! whose surface it entered, and an outbound drag starts from the surface of
//! the window that asked.
//!
//! On X11 (the display handle is Xlib/XCB, not Wayland) `attach` returns an
//! inert guard — such a window is routed to the sibling XDND backend
//! (`external_dnd/x11.rs`) by `UnixExternalDndBackend`, so this one never has
//! to answer for it.

use std::sync::Arc;

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use teksilo_core::raw_handle::ParentHandle;
use teksilo_core::window::TeksiloWindowId;
use teksilo_core::{AppEventPoster, DragImageData, OsDragId, OsDragStart, OutboundDragData};
use wayland_backend::client::ObjectId;
use wayland_client::Proxy;
use wayland_client::protocol::wl_surface::WlSurface;

use super::{ExternalDndBackend, ExternalDndGuard, NoopDndGuard};
use crate::wayland_data::WaylandDataHub;
use crate::wayland_data::dnd::{self, Route};

/// One window's registration on the hub. Dropping it withdraws the window's
/// surface; the device stays, for the other windows and the clipboard.
pub struct WaylandDndGuard {
    hub: Arc<WaylandDataHub>,
    window_id: TeksiloWindowId,
    poster: Arc<dyn AppEventPoster>,
    /// Protocol id of the surface this window published.
    route_key: u32,
}

impl Drop for WaylandDndGuard {
    fn drop(&mut self) {
        let key = self.route_key;
        if let Err(err) = self.hub.with_state(|state| state.dnd.remove_route(key)) {
            eprintln!("teksilo-platform: withdrawing a closed window from drag-and-drop: {err}");
        }
    }
}

impl ExternalDndGuard for WaylandDndGuard {
    fn begin_drag(
        &self,
        data: &OutboundDragData,
        _image: Option<&DragImageData>,
        pointer: teksilo_tokens::PointerKind,
    ) -> OsDragStart {
        // A device class that never pressed (a seat with no touch) cannot start
        // a drag: decline now, so the in-app drag carries on.
        let started = self.hub.with_state(|state| {
            if !state.press_serials.has_press_for(pointer) {
                return OsDragStart::Declined;
            }
            let drag = OsDragId::next();
            dnd::begin_outbound(
                state,
                self.window_id,
                &self.poster,
                data.clone(),
                pointer,
                drag,
            );
            // Confirmed (or cancelled) through the event loop, as the
            // framework expects of a backend that starts asynchronously.
            OsDragStart::Pending(drag)
        });
        started.unwrap_or_else(|err| {
            eprintln!("teksilo-platform: starting a drag: {err}");
            OsDragStart::Declined
        })
    }

    fn cancel_drag(&self) {
        if let Err(err) = self.hub.with_state(dnd::cancel_outbound) {
            eprintln!("teksilo-platform: cancelling a drag: {err}");
        }
    }

    fn set_drop_accepted(&self, accepted: bool) {
        if let Err(err) = self
            .hub
            .with_state(|state| dnd::revise_accept(state, accepted))
        {
            eprintln!("teksilo-platform: answering a drag: {err}");
        }
    }
}

/// Wayland external-drag backend. See the module docs.
#[derive(Default)]
pub struct WaylandExternalDndBackend;

impl WaylandExternalDndBackend {
    pub fn new() -> Self {
        Self
    }
}

/// The hub for `display`: the one the clipboard already made, or a new one on
/// the display handle the application installed.
fn hub_for(display: *mut std::ffi::c_void) -> Result<Arc<WaylandDataHub>, String> {
    if let Some(hub) = WaylandDataHub::existing(display as usize) {
        return Ok(hub);
    }
    let owned = crate::window::installed_display_handle()
        .ok_or_else(|| "no display handle was installed".to_string())?;
    let hub = WaylandDataHub::for_display(owned)?;
    if WaylandDataHub::existing(display as usize).is_none() {
        return Err("the window is on another display than the application's".into());
    }
    Ok(hub)
}

impl ExternalDndBackend for WaylandExternalDndBackend {
    fn attach(
        &mut self,
        parent: ParentHandle,
        window_id: TeksiloWindowId,
        poster: Arc<dyn AppEventPoster>,
    ) -> Box<dyn ExternalDndGuard> {
        // Wayland only — X11 falls through to the no-op.
        let RawDisplayHandle::Wayland(display) = parent.raw_display_handle() else {
            return Box::new(NoopDndGuard);
        };
        let RawWindowHandle::Wayland(window) = parent.raw_window_handle() else {
            return Box::new(NoopDndGuard);
        };
        let hub = match hub_for(display.display.as_ptr()) {
            Ok(hub) => hub,
            Err(err) => {
                eprintln!("teksilo-platform: drag-and-drop is unavailable on this window: {err}");
                return Box::new(NoopDndGuard);
            }
        };

        let registered = hub.with_state(|state| {
            // winit's surface, as a proxy on the hub's connection (the same
            // connection, so the same object).
            // SAFETY: `window.surface` is the live `wl_surface` of a window
            // winit created on this display, which outlives this guard.
            let id = unsafe {
                ObjectId::from_ptr(WlSurface::interface(), window.surface.as_ptr().cast())
            }
            .ok()?;
            let surface = WlSurface::from_id(state.conn(), id).ok()?;
            let key = surface.id().protocol_id();
            state.dnd.add_route(
                key,
                Route {
                    window_id,
                    poster: poster.clone(),
                    surface,
                },
            );
            Some(key)
        });
        match registered {
            Ok(Some(route_key)) => Box::new(WaylandDndGuard {
                hub,
                window_id,
                poster,
                route_key,
            }),
            Ok(None) => {
                eprintln!(
                    "teksilo-platform: drag-and-drop is unavailable on this window: its surface could not be resolved"
                );
                Box::new(NoopDndGuard)
            }
            Err(err) => {
                eprintln!("teksilo-platform: drag-and-drop is unavailable on this window: {err}");
                Box::new(NoopDndGuard)
            }
        }
    }
}
