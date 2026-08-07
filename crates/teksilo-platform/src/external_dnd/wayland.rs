// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Wayland external drag-and-drop backend (`wl_data_device`).
//!
//! winit (0.30) does not implement Wayland drag-and-drop, so there is no
//! competing `wl_data_device` to displace. We bind our own, on **winit's own
//! `wl_display`** via the system libwayland multi-queue model
//! ([`Backend::from_foreign_display`]): libwayland multiplexes events to
//! per-object event queues, so a dedicated dispatch thread reading our queue
//! coexists with winit's event loop on the same connection. Because the
//! connection is shared, the `wl_surface` object ids match winit's, so an
//! `enter` can be resolved back to the window it names.
//!
//! `wl_data_device` is bound per **seat**, not per surface. A client with
//! several windows therefore has a single drag stream, and the compositor
//! delivers it on **one** of the devices the client has bound — not on "the
//! one belonging to the window under the pointer", which is not a thing the
//! protocol expresses. So a device must not answer only for its own surface:
//! every window publishes its surface into a shared [`SharedDnd`] routing
//! table, and whichever thread receives the `enter` posts to the window that
//! table names. `motion` / `drop` / `leave` carry no surface and are gated on
//! the route the last `enter` established.
//!
//! (Filtering by own surface instead is invisible in a single-window app —
//! the only device is also the only window — and breaks every window but one
//! as soon as a second exists.)
//!
//! On X11 (the display handle is Xlib/XCB, not Wayland) `attach` returns an
//! inert guard — external OS drops are then a no-op and the `DropZone` Browse
//! button is the path.
//!
//! **Verification status:** compiled and exercised on a Linux/Wayland host
//! (`cfg(all(unix, not(target_os = "macos")))`); not built on the macOS
//! development machine.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::fd::{AsFd, FromRawFd, OwnedFd};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use teksilo_canvas::Point;
use teksilo_core::raw_handle::ParentHandle;
use teksilo_core::window::TeksiloWindowId;
use teksilo_core::{
    AppEventPoster, DragImageData, DropOutcome, ExternalDropData, OutboundDragData,
};

use wayland_backend::client::ObjectId;
use wayland_backend::sys::client::Backend;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_data_device::{Event as DataDeviceEvent, WlDataDevice};
use wayland_client::protocol::wl_data_device_manager::{DndAction, WlDataDeviceManager};
use wayland_client::protocol::wl_data_offer::{Event as DataOfferEvent, WlDataOffer};
use wayland_client::protocol::wl_data_source::{Event as DataSourceEvent, WlDataSource};
use wayland_client::protocol::wl_pointer::{ButtonState, Event as PointerEvent, WlPointer};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};

use super::{
    ExternalDndBackend, ExternalDndEventPayload, ExternalDndGuard, ExternalDragEvent, NoopDndGuard,
    outbound_bytes, outbound_mimes,
};

/// Command sent from the (main-thread) guard to the DnD dispatch thread to
/// start a native outbound drag. The wayland proxies all live on the dispatch
/// thread, so the request is handed across rather than touched directly.
enum OutboundCommand {
    Begin {
        data: OutboundDragData,
        #[allow(dead_code)] // reserved for a future caller-supplied drag icon surface
        image: Option<DragImageData>,
    },
}

/// Preferred drop MIME types, in order. `text/uri-list` carries file paths.
const PREFERRED_MIMES: &[&str] = &["text/uri-list", "text/plain;charset=utf-8", "text/plain"];

/// Where to deliver the events of a drag hovering a given surface.
#[derive(Clone)]
struct Route {
    window_id: TeksiloWindowId,
    poster: Arc<dyn AppEventPoster>,
}

/// Which dispatch thread is servicing the in-flight drag, and the `enter`
/// serial that identifies it.
#[derive(Clone, Copy)]
struct Claim {
    device: TeksiloWindowId,
    serial: u32,
}

/// App-global drag state shared by every window's dispatch thread.
///
/// `wl_data_device` is bound per **seat**, not per surface, so a client with
/// several windows still gets a single drag stream — and the compositor is free
/// to deliver it on any one of the devices the client has bound, whichever
/// window is actually under the pointer. A device that only recognised its own
/// surface would therefore discard every drag aimed at a sibling window, which
/// is invisible to a single-window app and total breakage for a multi-window
/// one. Routing is by surface instead: any device can deliver to any window.
#[derive(Default)]
struct SharedDnd {
    /// Surface protocol id → the window that owns it. Keyed by protocol id
    /// rather than `ObjectId` because ids reach us from sibling windows'
    /// connection wrappers, and the numeric id is what is stable across them.
    routes: Mutex<HashMap<u32, Route>>,
    /// Set while a drag is being serviced, so a compositor that fans the same
    /// `enter` out to every bound device can't have two threads deliver it —
    /// and drop it twice.
    claim: Mutex<Option<Claim>>,
}

impl SharedDnd {
    /// Resolve the window owning `surface`, if it is one of ours.
    fn route_for(&self, surface: &ObjectId) -> Option<Route> {
        self.routes
            .lock()
            .ok()?
            .get(&surface.protocol_id())
            .cloned()
    }

    /// Try to become the thread that services the drag identified by `serial`.
    /// The first caller wins; a repeat from the winner stays true.
    fn claim_drag(&self, serial: u32, device: TeksiloWindowId) -> bool {
        let Ok(mut claim) = self.claim.lock() else {
            return true;
        };
        match *claim {
            Some(existing) if existing.serial == serial => existing.device == device,
            _ => {
                *claim = Some(Claim { device, serial });
                true
            }
        }
    }

    /// Release the claim held by `device`, so the next drag is contestable.
    fn release_claim(&self, device: TeksiloWindowId) {
        if let Ok(mut claim) = self.claim.lock()
            && claim.is_some_and(|c| c.device == device)
        {
            *claim = None;
        }
    }
}

/// Dispatch-thread state.
struct DndState {
    window_id: TeksiloWindowId,
    poster: Arc<dyn AppEventPoster>,
    conn: Connection,
    qh: QueueHandle<DndState>,
    /// Routing table + drag claim shared with every other window's thread.
    shared: Arc<SharedDnd>,
    /// Where this thread is delivering the in-flight drag — the window owning
    /// the surface the last `enter` named, which is often *not* this thread's
    /// own window. `None` when no drag is being serviced here, which is what
    /// gates `motion` / `drop` / `leave` (they carry no surface of their own).
    active_route: Option<Route>,
    /// The current drag's data offer + its advertised MIME types.
    current_offer: Option<WlDataOffer>,
    offer_mimes: Vec<String>,
    position: Point,
    // --- Outbound (app → OS) state ---
    /// Manager kept alive so we can create data sources on demand.
    data_device_manager: WlDataDeviceManager,
    /// The data device used to initiate `start_drag`.
    data_device: WlDataDevice,
    /// This window's surface as a proxy (drag origin for `start_drag`).
    origin_surface: Option<WlSurface>,
    /// Serial of the most recent pointer button *press* — required by
    /// `start_drag` (must come from a button-down in the implicit grab).
    last_press_serial: u32,
    /// The in-flight outbound data source + the bytes it serves on `send`.
    outbound_source: Option<WlDataSource>,
    outbound_data: Option<OutboundDragData>,
    /// Last negotiated drag action (copy / move) for the outbound drag.
    outbound_action: DndAction,
    /// Set once the drop has been performed, so a trailing `cancelled` does
    /// not override the success outcome.
    outbound_finished: bool,
    /// Inbound commands from the guard (start an outbound drag).
    cmd_rx: Receiver<OutboundCommand>,
    // Held to keep the proxies alive for the queue's lifetime.
    _seat: WlSeat,
    _pointer: WlPointer,
}

impl DndState {
    /// Post to *this* thread's own window — outbound-drag outcomes, which
    /// belong to the window that started the drag.
    fn post(&self, event: ExternalDragEvent) {
        self.poster.post_external(Box::new(ExternalDndEventPayload {
            window_id_owner: self.window_id,
            event,
        }));
    }

    /// Post to the window under the pointer, which any thread may be
    /// servicing. No-op when no drag is being serviced here.
    fn post_routed(&self, event: ExternalDragEvent) {
        let Some(route) = &self.active_route else {
            return;
        };
        route
            .poster
            .post_external(Box::new(ExternalDndEventPayload {
                window_id_owner: route.window_id,
                event,
            }));
    }

    /// Drain pending outbound-drag commands from the guard and start a native
    /// `wl_data_source` drag for each. Called once per dispatch-loop tick.
    fn process_outbound_commands(&mut self) {
        while let Ok(cmd) = self.cmd_rx.try_recv() {
            match cmd {
                OutboundCommand::Begin { data, image: _ } => self.begin_outbound(data),
            }
        }
    }

    fn begin_outbound(&mut self, data: OutboundDragData) {
        let Some(origin) = self.origin_surface.clone() else {
            // No surface proxy ⇒ can't start a drag; report cancellation so
            // the source's on_drag_ended still fires.
            self.post(ExternalDragEvent::DragEnded {
                outcome: DropOutcome::Cancelled,
            });
            return;
        };
        // `start_drag` requires a serial from a recent pointer button press in
        // the current implicit grab. If we haven't observed one yet (e.g. the
        // press hadn't been dispatched on this thread when the begin command
        // arrived), the compositor would silently reject the request and never
        // send a terminal event — leaving the in-app drag dead and the stash
        // leaked. Report cancellation instead so the framework cleans up.
        if self.last_press_serial == 0 {
            self.post(ExternalDragEvent::DragEnded {
                outcome: DropOutcome::Cancelled,
            });
            return;
        }
        // Tear down any previous in-flight source.
        if let Some(src) = self.outbound_source.take() {
            src.destroy();
        }

        let source = self.data_device_manager.create_data_source(&self.qh, ());
        for mime in outbound_mimes(&data) {
            source.offer(mime);
        }
        if source.version() >= 3 {
            // Copy only — never advertise Move, which would let the
            // destination physically relocate a dragged file. Move-out should
            // be an explicit opt-in, not the baseline behavior.
            source.set_actions(DndAction::Copy);
        }
        self.data_device.start_drag(
            Some(&source),
            &origin,
            None, // no custom drag icon surface yet
            self.last_press_serial,
        );
        let _ = self.conn.flush();

        self.outbound_data = Some(data);
        self.outbound_source = Some(source);
        self.outbound_action = DndAction::empty();
        self.outbound_finished = false;
    }

    /// Serve the bytes for `mime_type` over `fd` in response to a
    /// `wl_data_source::send` event.
    fn serve_send(&self, mime_type: &str, fd: OwnedFd) {
        let Some(data) = &self.outbound_data else {
            return;
        };
        let bytes = outbound_bytes(data, mime_type);
        let mut file = std::fs::File::from(fd);
        let _ = file.write_all(&bytes);
        // Dropping `file` closes the write end so the reader sees EOF.
    }

    fn finish_outbound(&mut self, outcome: DropOutcome) {
        if let Some(src) = self.outbound_source.take() {
            src.destroy();
        }
        self.outbound_data = None;
        self.post(ExternalDragEvent::DragEnded { outcome });
    }
}

// `outbound_mimes` / `outbound_bytes` are shared with the X11 backend from
// `super` — an app must export the same bytes under the same MIME types
// whichever Linux display server it happens to be running on.

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

impl Dispatch<WlPointer, ()> for DndState {
    fn event(
        state: &mut Self,
        _pointer: &WlPointer,
        event: PointerEvent,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Capture the serial of the most recent button *press*. `start_drag`
        // requires a serial from a button-down event in the current implicit
        // grab; the most recent press is the one that began the drag.
        if let PointerEvent::Button {
            serial, state: btn, ..
        } = event
            && btn == WEnum::Value(ButtonState::Pressed)
        {
            state.last_press_serial = serial;
        }
    }
}

impl Dispatch<WlDataSource, ()> for DndState {
    fn event(
        state: &mut Self,
        _source: &WlDataSource,
        event: DataSourceEvent,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            // The compositor asks us to provide the data for a MIME type by
            // writing into `fd`.
            DataSourceEvent::Send { mime_type, fd } => {
                state.serve_send(&mime_type, fd);
                let _ = state.conn.flush();
            }
            // The negotiated drag action (copy / move). Track for the outcome.
            DataSourceEvent::Action {
                dnd_action: WEnum::Value(action),
            } => {
                state.outbound_action = action;
            }
            // The user released over a valid target.
            DataSourceEvent::DndDropPerformed => {
                state.outbound_finished = true;
            }
            // The destination finished reading: success. Map the negotiated
            // action to copy / move.
            DataSourceEvent::DndFinished => {
                let outcome = if state.outbound_action.contains(DndAction::Move) {
                    DropOutcome::OsMove
                } else {
                    DropOutcome::OsCopy
                };
                state.finish_outbound(outcome);
            }
            // `cancelled` means the source is no longer valid. If it arrives
            // *before* a drop was performed, the drag was rejected/aborted →
            // Cancelled. If it arrives *after* `dnd_drop_performed` (some
            // compositors send `cancelled` rather than `dnd_finished` as the
            // terminal once the target has taken the data), still report
            // success based on the negotiated action — otherwise no terminal
            // `DragEnded` would ever fire and the stash would leak.
            DataSourceEvent::Cancelled => {
                if state.outbound_finished {
                    let outcome = if state.outbound_action.contains(DndAction::Move) {
                        DropOutcome::OsMove
                    } else {
                        DropOutcome::OsCopy
                    };
                    state.finish_outbound(outcome);
                } else {
                    state.finish_outbound(DropOutcome::Cancelled);
                }
            }
            _ => {}
        }
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
                // Deliver to whichever of our windows owns the entered surface —
                // not necessarily the one whose device received the event.
                let route = state.shared.route_for(&surface.id());
                let Some(route) = route else {
                    // Not one of our surfaces (a foreign window, or one already
                    // detached): leave it to whoever owns it.
                    state.active_route = None;
                    return;
                };
                if !state.shared.claim_drag(serial, state.window_id) {
                    // Another thread is already servicing this drag.
                    state.active_route = None;
                    return;
                }
                state.active_route = Some(route);
                state.position = Point::new(x as f32, y as f32);
                // Accept a MIME we can read AND negotiate a drag action — both
                // are required or the compositor shows the "forbidden" cursor
                // and blocks the drop. `set_actions` is a v3+ request.
                if let Some(offer) = &id {
                    if let Some(mime) = pick_mime(&state.offer_mimes) {
                        offer.accept(serial, Some(mime));
                    }
                    if offer.version() >= 3 {
                        offer.set_actions(DndAction::Copy, DndAction::Copy);
                    }
                }
                // Bytes aren't available until drop; advertise the offered MIME
                // types so the drop target can decide accept/reject on hover.
                state.post_routed(ExternalDragEvent::Entered {
                    data: ExternalDropData {
                        formats: state.offer_mimes.clone(),
                        ..Default::default()
                    },
                    position: state.position,
                });
            }
            DataDeviceEvent::Motion { x, y, .. } => {
                if state.active_route.is_none() {
                    return;
                }
                state.position = Point::new(x as f32, y as f32);
                state.post_routed(ExternalDragEvent::Moved {
                    position: state.position,
                });
            }
            DataDeviceEvent::Leave => {
                state.post_routed(ExternalDragEvent::Left);
                state.active_route = None;
                state.shared.release_claim(state.window_id);
                state.current_offer = None;
            }
            DataDeviceEvent::Drop => {
                if state.active_route.is_none() {
                    return;
                }
                // Self-drag: this is an app-originated OS drag (we hold the
                // source) re-entering and dropping on our own window. We must
                // NOT pipe-read the payload here — the bytes would have to come
                // from our own `wl_data_source.send` event, which is queued on
                // *this* dispatch thread and so can never be serviced while we
                // block in `receive()` (a self-deadlock). teksilo-core recovers
                // the original typed payload from its stash, so the dropped
                // bytes aren't needed.
                let data = if state.outbound_source.is_some() {
                    ExternalDropData::default()
                } else {
                    state
                        .current_offer
                        .as_ref()
                        .map(|offer| receive(&state.conn, offer, &state.offer_mimes))
                        .unwrap_or_default()
                };
                state.post_routed(ExternalDragEvent::Dropped {
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
                state.active_route = None;
                state.shared.release_claim(state.window_id);
            }
            // Clipboard selection — not our concern.
            DataDeviceEvent::Selection { .. } => {}
            _ => {}
        }
    }

    // The `data_offer` event (opcode 0) creates a new `wl_data_offer` child
    // object; tell wayland-client its interface + user-data so it can build the
    // proxy. Without this it panics ("Missing event_created_child specialization").
    wayland_client::event_created_child!(DndState, WlDataDevice, [
        wayland_client::protocol::wl_data_device::EVT_DATA_OFFER_OPCODE => (WlDataOffer, ()),
    ]);
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
///
/// Holds the outbound-command sender so [`ExternalDndGuard::begin_drag`] can
/// hand a start-drag request to the dispatch thread (all wayland proxies live
/// there).
pub struct WaylandDndGuard {
    cmd_tx: Option<Sender<OutboundCommand>>,
    /// Shared routing table, so closing this window withdraws its surface.
    shared: Arc<SharedDnd>,
    /// Protocol id of the surface this window published, if any.
    route_key: Option<u32>,
    window_id: TeksiloWindowId,
}

/// Withdraw the window's surface from the shared routing table. A stale entry
/// would send a later drag to a window that no longer exists — and Wayland
/// recycles protocol ids, so the id could even be re-issued to another
/// window's surface.
impl Drop for WaylandDndGuard {
    fn drop(&mut self) {
        if let Some(key) = self.route_key
            && let Ok(mut routes) = self.shared.routes.lock()
        {
            routes.remove(&key);
        }
        self.shared.release_claim(self.window_id);
    }
}

impl ExternalDndGuard for WaylandDndGuard {
    fn begin_drag(&self, data: &OutboundDragData, image: Option<&DragImageData>) -> bool {
        let Some(tx) = &self.cmd_tx else {
            return false;
        };
        // The dispatch thread performs create_data_source + start_drag on its
        // next tick (≤ 8 ms) using the most recent button-press serial, which
        // is still valid because the button is held throughout the drag.
        tx.send(OutboundCommand::Begin {
            data: data.clone(),
            image: image.cloned(),
        })
        .is_ok()
    }
}

/// Wayland external-drag backend. See the module docs.
///
/// One per app, so every window's dispatch thread shares a single surface
/// routing table.
#[derive(Default)]
pub struct WaylandExternalDndBackend {
    shared: Arc<SharedDnd>,
}

impl WaylandExternalDndBackend {
    pub fn new() -> Self {
        Self::default()
    }
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
        // Bind the seat pointer on our own connection to observe button-press
        // serials (start_drag needs one). Same multi-queue model as the rest
        // of this backend — we never read the socket ourselves.
        let pointer = seat.get_pointer(&qh, ());

        // winit's surface id (same connection ⇒ comparable to `enter.surface`).
        let target_surface = unsafe {
            ObjectId::from_ptr(WlSurface::interface(), window.surface.as_ptr() as *mut _)
        }
        .ok();
        // A `WlSurface` proxy for the same surface — the drag origin.
        let origin_surface = target_surface
            .clone()
            .and_then(|id| WlSurface::from_id(&conn, id).ok());

        // Publish this window's surface so *any* window's dispatch thread can
        // deliver a drag that lands on it.
        let route_key = target_surface.as_ref().map(ObjectId::protocol_id);
        if let Some(key) = route_key
            && let Ok(mut routes) = self.shared.routes.lock()
        {
            routes.insert(
                key,
                Route {
                    window_id,
                    poster: poster.clone(),
                },
            );
        }

        let (cmd_tx, cmd_rx) = channel::<OutboundCommand>();

        let mut state = DndState {
            window_id,
            poster,
            conn,
            qh: qh.clone(),
            shared: self.shared.clone(),
            active_route: None,
            current_offer: None,
            offer_mimes: Vec::new(),
            position: Point::new(0.0, 0.0),
            data_device_manager: ddm,
            data_device,
            origin_surface,
            last_press_serial: 0,
            outbound_source: None,
            outbound_data: None,
            outbound_action: DndAction::empty(),
            outbound_finished: false,
            cmd_rx,
            _seat: seat,
            _pointer: pointer,
        };

        std::thread::Builder::new()
            .name(format!("teksilo-wayland-dnd-{}", window_id.raw()))
            .spawn(move || {
                // CRITICAL: never read the socket here. winit's event loop is
                // the sole reader of this shared `wl_display`; a second reader
                // (`blocking_dispatch` → `prepare_read`/`read_events`) aborts
                // the process. libwayland's multi-queue model buffers events
                // for our objects whenever *anyone* reads the socket, so we
                // only drain our queue with `dispatch_pending` (no read) and
                // poll on a short interval. Drag events arrive within one tick.
                while queue.dispatch_pending(&mut state).is_ok() {
                    // Start any outbound drags the guard requested.
                    state.process_outbound_commands();
                    // Flush any requests we queued (accept / receive / finish /
                    // offer / start_drag).
                    let _ = state.conn.flush();
                    std::thread::sleep(std::time::Duration::from_millis(8));
                }
            })
            .ok();

        Box::new(WaylandDndGuard {
            cmd_tx: Some(cmd_tx),
            shared: self.shared.clone(),
            route_key,
            window_id,
        })
    }
}
