//! Wayland external drag-and-drop backend (`wl_data_device`).
//!
//! winit (0.30) does not implement Wayland drag-and-drop, so there is no
//! competing `wl_data_device` to displace. We bind our own, on **winit's own
//! `wl_display`** via the system libwayland multi-queue model
//! ([`Backend::from_foreign_display`]): libwayland multiplexes events to
//! per-object event queues, so a dedicated dispatch thread reading our queue
//! coexists with winit's event loop on the same connection. Because the
//! connection is shared, the `wl_surface` object ids match winit's, so we can
//! filter `enter` events to this window's surface.
//!
//! `wl_data_device` is per-seat (app-global), so each window's backend filters
//! by its own surface; `motion` / `drop` / `leave` (which carry no surface) are
//! gated on whether the most recent `enter` matched.
//!
//! On X11 (the display handle is Xlib/XCB, not Wayland) `attach` returns an
//! inert guard — external OS drops are then a no-op and the `DropZone` Browse
//! button is the path.
//!
//! **Verification status:** compiled and exercised on a Linux/Wayland host
//! (`cfg(all(unix, not(target_os = "macos")))`); not built on the macOS
//! development machine.

use std::io::Read;
use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use std::sync::Arc;

use bastyde_canvas::Point;
use bastyde_core::raw_handle::ParentHandle;
use bastyde_core::window::BastydeWindowId;
use bastyde_core::{AppEventPoster, ExternalDropData};
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};

use wayland_backend::client::ObjectId;
use wayland_backend::sys::client::Backend;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_data_device::{Event as DataDeviceEvent, WlDataDevice};
use wayland_client::protocol::wl_data_device_manager::WlDataDeviceManager;
use wayland_client::protocol::wl_data_offer::{Event as DataOfferEvent, WlDataOffer};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};

use super::{
    ExternalDndBackend, ExternalDndEventPayload, ExternalDndGuard, ExternalDragEvent, NoopDndGuard,
};

/// Preferred drop MIME types, in order. `text/uri-list` carries file paths.
const PREFERRED_MIMES: &[&str] = &["text/uri-list", "text/plain;charset=utf-8", "text/plain"];

/// Dispatch-thread state.
struct DndState {
    window_id: BastydeWindowId,
    poster: Arc<dyn AppEventPoster>,
    conn: Connection,
    /// winit's surface, to filter `enter` to this window.
    target_surface: Option<ObjectId>,
    /// True while a drag's most recent `enter` matched our surface.
    active: bool,
    /// The current drag's data offer + its advertised MIME types.
    current_offer: Option<WlDataOffer>,
    offer_mimes: Vec<String>,
    position: Point,
    // Held to keep the proxies alive for the queue's lifetime.
    _seat: WlSeat,
    _data_device: WlDataDevice,
}

impl DndState {
    fn post(&self, event: ExternalDragEvent) {
        self.poster.post_external(Box::new(ExternalDndEventPayload {
            window_id_owner: self.window_id,
            event,
        }));
    }
}

// The registry is driven by `GlobalList`; this impl is just the required bound.
impl Dispatch<WlRegistry, GlobalListContents> for DndState {
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

impl Dispatch<WlSeat, ()> for DndState {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: <WlSeat as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlDataDeviceManager, ()> for DndState {
    fn event(
        _: &mut Self,
        _: &WlDataDeviceManager,
        _: <WlDataDeviceManager as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlDataOffer, ()> for DndState {
    fn event(
        state: &mut Self,
        _offer: &WlDataOffer,
        event: DataOfferEvent,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let DataOfferEvent::Offer { mime_type } = event {
            state.offer_mimes.push(mime_type);
        }
    }
}

impl Dispatch<WlDataDevice, ()> for DndState {
    fn event(
        state: &mut Self,
        _device: &WlDataDevice,
        event: DataDeviceEvent,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            // A new drag's offer is introduced before `enter`. Reset MIME list.
            DataDeviceEvent::DataOffer { id } => {
                state.current_offer = Some(id);
                state.offer_mimes.clear();
            }
            DataDeviceEvent::Enter {
                serial,
                surface,
                x,
                y,
                id,
            } => {
                let matches = state
                    .target_surface
                    .as_ref()
                    .map(|t| &surface.id() == t)
                    .unwrap_or(true);
                state.active = matches;
                if !matches {
                    return;
                }
                state.position = Point::new(x as f32, y as f32);
                // Accept a MIME we can read so the compositor allows the drop.
                if let Some(offer) = &id {
                    if let Some(mime) = pick_mime(&state.offer_mimes) {
                        offer.accept(serial, Some(mime));
                    }
                }
                state.post(ExternalDragEvent::Entered {
                    data: ExternalDropData::default(),
                    position: state.position,
                });
            }
            DataDeviceEvent::Motion { x, y, .. } => {
                if !state.active {
                    return;
                }
                state.position = Point::new(x as f32, y as f32);
                state.post(ExternalDragEvent::Moved {
                    position: state.position,
                });
            }
            DataDeviceEvent::Leave => {
                if state.active {
                    state.post(ExternalDragEvent::Left);
                }
                state.active = false;
                state.current_offer = None;
            }
            DataDeviceEvent::Drop => {
                if !state.active {
                    return;
                }
                let data = state
                    .current_offer
                    .as_ref()
                    .map(|offer| receive(&state.conn, offer, &state.offer_mimes))
                    .unwrap_or_default();
                state.post(ExternalDragEvent::Dropped {
                    data,
                    position: state.position,
                });
                if let Some(offer) = state.current_offer.take() {
                    // `finish` requires version >= 3; `destroy` is always safe.
                    if offer.version() >= 3 {
                        offer.finish();
                    }
                    offer.destroy();
                }
                state.active = false;
            }
            // Clipboard selection — not our concern.
            DataDeviceEvent::Selection { .. } => {}
            _ => {}
        }
    }
}

/// Pick the best MIME type we can decode from the offered set.
fn pick_mime(offered: &[String]) -> Option<String> {
    for pref in PREFERRED_MIMES {
        if let Some(m) = offered.iter().find(|m| m.as_str() == *pref) {
            return Some(m.clone());
        }
    }
    None
}

/// Receive the drop payload for the best offered MIME over a pipe.
fn receive(conn: &Connection, offer: &WlDataOffer, mimes: &[String]) -> ExternalDropData {
    let Some(mime) = pick_mime(mimes) else {
        return ExternalDropData::default();
    };

    // Pipe: compositor writes the data into `write`, we read from `read`.
    let mut fds = [0i32; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return ExternalDropData::default();
    }
    let read_fd = unsafe { OwnedFd::from_raw_fd(fds[0]) };
    let write_fd = unsafe { OwnedFd::from_raw_fd(fds[1]) };

    offer.receive(mime.clone(), write_fd.as_fd());
    // Flush so the request reaches the compositor, then drop our write end so
    // the read sees EOF once the compositor finishes writing.
    let _ = conn.flush();
    drop(write_fd);

    let mut buf = Vec::new();
    let mut file = std::fs::File::from(read_fd);
    let _ = file.read_to_end(&mut buf);
    let text = String::from_utf8_lossy(&buf).into_owned();

    if mime == "text/uri-list" {
        ExternalDropData::from_uri_list(&text)
    } else {
        ExternalDropData {
            text: Some(text),
            ..Default::default()
        }
    }
}

/// Guard: the dispatch thread runs until the connection breaks (window close
/// drops winit's display and the next `blocking_dispatch` errors out). There is
/// nothing to actively revoke — the per-window `wl_data_device` is released
/// when its proxies are dropped with the thread's state.
pub struct WaylandDndGuard;

impl ExternalDndGuard for WaylandDndGuard {}

/// Wayland external-drag backend. See the module docs.
#[derive(Default)]
pub struct WaylandExternalDndBackend;

impl WaylandExternalDndBackend {
    pub fn new() -> Self {
        Self
    }
}

impl ExternalDndBackend for WaylandExternalDndBackend {
    fn attach(
        &mut self,
        parent: ParentHandle,
        window_id: BastydeWindowId,
        poster: Arc<dyn AppEventPoster>,
    ) -> Box<dyn ExternalDndGuard> {
        // Wayland only — X11 falls through to the no-op.
        let RawDisplayHandle::Wayland(display) = parent.raw_display_handle() else {
            return Box::new(NoopDndGuard);
        };
        let RawWindowHandle::Wayland(window) = parent.raw_window_handle() else {
            return Box::new(NoopDndGuard);
        };

        // Wrap winit's existing wl_display (shared connection, shared id space).
        let backend = unsafe { Backend::from_foreign_display(display.display.as_ptr() as *mut _) };
        let conn = Connection::from_backend(backend);

        let Ok((globals, mut queue)) = registry_queue_init::<DndState>(&conn) else {
            return Box::new(NoopDndGuard);
        };
        let qh = queue.handle();

        let Ok(seat) = globals.bind::<WlSeat, _, _>(&qh, 1..=5, ()) else {
            return Box::new(NoopDndGuard);
        };
        let Ok(ddm) = globals.bind::<WlDataDeviceManager, _, _>(&qh, 1..=3, ()) else {
            return Box::new(NoopDndGuard);
        };
        let data_device = ddm.get_data_device(&seat, &qh, ());

        // winit's surface id (same connection ⇒ comparable to `enter.surface`).
        let target_surface =
            unsafe { ObjectId::from_ptr(WlSurface::interface(), window.surface.as_ptr() as *mut _) }
                .ok();

        let mut state = DndState {
            window_id,
            poster,
            conn,
            target_surface,
            active: false,
            current_offer: None,
            offer_mimes: Vec::new(),
            position: Point::new(0.0, 0.0),
            _seat: seat,
            _data_device: data_device,
        };

        std::thread::Builder::new()
            .name(format!("bastyde-wayland-dnd-{}", window_id.raw()))
            .spawn(move || {
                // Coexists with winit's loop: libwayland multiplexes events to
                // our queue. Exits when the connection breaks (window/app gone).
                while queue.blocking_dispatch(&mut state).is_ok() {}
            })
            .ok();

        Box::new(WaylandDndGuard)
    }
}
