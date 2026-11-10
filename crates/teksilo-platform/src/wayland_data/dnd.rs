// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! External drag-and-drop over the hub's one `wl_data_device`.
//!
//! The device is the seat's, not a window's, so a drag aimed at any of our
//! windows arrives on it. Each window publishes its surface in
//! [`DndState::routes`]; an `enter` names a surface and is delivered to the
//! window that owns it. `motion` / `drop` / `leave` carry no surface and follow
//! the route the last `enter` established.
//!
//! Outbound drags (app → OS) are started on the same device with the press
//! serial of the device class carrying the drag ([`TouchSerialSource`]).
//!
//! [`TouchSerialSource`]: crate::external_dnd::TouchSerialSource

use std::collections::HashMap;
use std::sync::Arc;

use teksilo_canvas::Point;
use teksilo_core::window::TeksiloWindowId;
use teksilo_core::{AppEventPoster, DropOutcome, ExternalDropData, OsDragId, OutboundDragData};
use wayland_client::protocol::wl_data_device::Event as DataDeviceEvent;
use wayland_client::protocol::wl_data_device_manager::DndAction;
use wayland_client::protocol::wl_data_offer::WlDataOffer;
use wayland_client::protocol::wl_data_source::{Event as DataSourceEvent, WlDataSource};
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Proxy, WEnum};

use super::{HubState, SourceKind, io, offer_mimes};
use crate::external_dnd::{
    ExternalDndEventPayload, ExternalDragEvent, outbound_bytes, outbound_mimes,
};

/// Preferred drop MIME types, in order. `text/uri-list` carries file paths.
const PREFERRED_MIMES: &[&str] = &["text/uri-list", "text/plain;charset=utf-8", "text/plain"];

/// Limits for reading a drop: on a thread of its own, so longer than a paste,
/// but a drop of a file list or some text is never large.
const DROP_LIMITS: io::Limits = io::Limits {
    idle: std::time::Duration::from_secs(5),
    total: std::time::Duration::from_secs(30),
    max_bytes: 64 * 1024 * 1024,
};

/// Where a window's drag events go.
#[derive(Clone)]
pub(crate) struct Route {
    pub(crate) window_id: TeksiloWindowId,
    pub(crate) poster: Arc<dyn AppEventPoster>,
    /// The window's surface, the origin of a drag it starts.
    pub(crate) surface: WlSurface,
}

impl Route {
    fn post(&self, event: ExternalDragEvent) {
        self.poster.post_external(Box::new(ExternalDndEventPayload {
            window_id_owner: self.window_id,
            event,
        }));
    }
}

/// An outbound drag in flight.
struct Outbound {
    drag: OsDragId,
    /// The window that started it, which hears how it ended.
    owner: Route,
    source: WlDataSource,
    data: OutboundDragData,
    /// Last negotiated action (copy / move).
    action: DndAction,
    /// Set once the drop is performed, so a trailing `cancelled` does not
    /// override the success outcome.
    dropped: bool,
}

/// Drag-and-drop state, part of [`HubState`].
#[derive(Default)]
pub(crate) struct DndState {
    /// Surface protocol id → the window that owns it.
    routes: HashMap<u32, Route>,
    /// Where the in-flight inbound drag is being delivered.
    active_route: Option<Route>,
    /// The in-flight inbound drag's offer.
    offer: Option<WlDataOffer>,
    /// The `enter` serial of that drag, needed by `accept` and its revisions.
    offer_serial: u32,
    /// The MIME type accepted for it, re-offered when the answer is revised.
    offer_accepted_mime: Option<String>,
    position: Point,
    outbound: Option<Outbound>,
}

impl DndState {
    /// Publish a window's surface, keyed by its protocol id.
    pub(crate) fn add_route(&mut self, key: u32, route: Route) {
        self.routes.insert(key, route);
    }

    /// Withdraw a window's surface. A stale entry would send a later drag to a
    /// window that no longer exists, and Wayland recycles protocol ids.
    pub(crate) fn remove_route(&mut self, key: u32) {
        if let Some(route) = self.routes.remove(&key)
            && self
                .active_route
                .as_ref()
                .is_some_and(|active| active.window_id == route.window_id)
        {
            self.active_route = None;
        }
    }

    fn route_for_window(&self, window_id: TeksiloWindowId) -> Option<&Route> {
        self.routes.values().find(|r| r.window_id == window_id)
    }

    /// Post to the window under the drag. No-op when none is being serviced.
    fn post_routed(&self, event: ExternalDragEvent) {
        if let Some(route) = &self.active_route {
            route.post(event);
        }
    }
}

/// Re-answer the in-flight inbound offer with the widget tree's verdict.
///
/// `accept` and `set_actions` may both be re-issued for the life of the offer,
/// and the compositor's cursor follows the latest answer. A refusal is
/// `accept(serial, None)` plus empty actions: `accept` alone leaves a
/// negotiated action standing and some compositors keep showing the copy
/// cursor, and empty actions alone leave the type accepted.
pub(crate) fn revise_accept(state: &mut HubState, accepted: bool) {
    let dnd = &state.dnd;
    let Some(offer) = dnd.offer.clone() else {
        return;
    };
    if accepted {
        offer.accept(dnd.offer_serial, dnd.offer_accepted_mime.clone());
        if offer.version() >= 3 {
            offer.set_actions(DndAction::Copy, DndAction::Copy);
        }
    } else {
        offer.accept(dnd.offer_serial, None);
        if offer.version() >= 3 {
            offer.set_actions(DndAction::empty(), DndAction::empty());
        }
    }
}

/// Start an outbound drag from `window_id`'s surface.
///
/// The framework is told the drag started **before** the request leaves: the
/// compositor answers `start_drag` with an `enter` on whichever of our surfaces
/// is under the pointer, and the framework must have handed its in-app session
/// over by then. Both go through the one event loop, so posting first orders
/// them.
pub(crate) fn begin_outbound(
    state: &mut HubState,
    window_id: TeksiloWindowId,
    poster: &Arc<dyn AppEventPoster>,
    data: OutboundDragData,
    pointer: teksilo_tokens::PointerKind,
    drag: OsDragId,
) {
    let cancelled = ExternalDragEvent::DragEnded {
        outcome: DropOutcome::Cancelled,
        drag: Some(drag),
    };
    let Some(owner) = state.dnd.route_for_window(window_id).cloned() else {
        // No surface to drag from: the guard declines up front, so this only
        // makes sure the waiting drag still ends.
        poster.post_external(Box::new(ExternalDndEventPayload {
            window_id_owner: window_id,
            event: cancelled,
        }));
        return;
    };
    // `start_drag` needs the serial of the press that opened the current
    // implicit grab — a `wl_pointer::button` for a mouse or pen, a
    // `wl_touch::down` for a finger. Without one held, the compositor would
    // refuse silently and never send a terminal event; report the
    // cancellation instead. Taking the serial also stops counting its press as
    // held: once the grab is converted, its release goes to the compositor's
    // drag grab and never reaches us.
    let Some(serial) = state.press_serials.take_for_drag(pointer) else {
        owner.post(cancelled);
        return;
    };
    // A drag still in flight is over: a seat carries one at a time.
    if let Some(previous) = state.dnd.outbound.take() {
        previous.source.destroy();
        previous.owner.post(ExternalDragEvent::DragEnded {
            outcome: DropOutcome::Cancelled,
            drag: Some(previous.drag),
        });
    }

    owner.post(ExternalDragEvent::DragStarted { drag });

    let source = state
        .manager
        .create_data_source(&state.qh, SourceKind::Drag);
    for mime in outbound_mimes(&data) {
        source.offer(mime);
    }
    if source.version() >= 3 {
        // Copy only — never advertise Move, which would let the destination
        // physically relocate a dragged file. Move-out is an explicit opt-in.
        source.set_actions(DndAction::Copy);
    }
    state
        .device
        .start_drag(Some(&source), &owner.surface, None, serial);
    state.dnd.outbound = Some(Outbound {
        drag,
        owner,
        source,
        data,
        action: DndAction::empty(),
        dropped: false,
    });
}

/// End the outbound drag at the framework's request (a release before the
/// start was confirmed, or Escape). Destroying the source ends a drag the
/// compositor started; with no source there is nothing to end.
pub(crate) fn cancel_outbound(state: &mut HubState) {
    if state.dnd.outbound.as_ref().is_some_and(|o| !o.dropped) {
        finish_outbound(state, DropOutcome::Cancelled);
    }
}

fn finish_outbound(state: &mut HubState, outcome: DropOutcome) {
    let Some(outbound) = state.dnd.outbound.take() else {
        return;
    };
    outbound.source.destroy();
    // Our own drag ending while it is over one of our windows is the one abort
    // a destination can be told about: the window holding the re-entered
    // session gets no `leave` for a drag the compositor has simply stopped. A
    // live route here can only belong to this drag — a seat carries one drag
    // at a time.
    if state.dnd.active_route.is_some() {
        state.dnd.post_routed(ExternalDragEvent::Cancelled);
        state.dnd.active_route = None;
    }
    outbound.owner.post(ExternalDragEvent::DragEnded {
        outcome,
        drag: Some(outbound.drag),
    });
}

fn outcome_of(action: DndAction) -> DropOutcome {
    if action.contains(DndAction::Move) {
        DropOutcome::OsMove
    } else {
        DropOutcome::OsCopy
    }
}

/// An event on our outbound drag's source.
pub(super) fn source_event(state: &mut HubState, source: &WlDataSource, event: DataSourceEvent) {
    let ours = state
        .dnd
        .outbound
        .as_ref()
        .is_some_and(|o| &o.source == source);
    if !ours {
        // A replaced drag's source: nothing waits on it any more.
        if matches!(event, DataSourceEvent::Cancelled) {
            source.destroy();
        }
        return;
    }
    match event {
        DataSourceEvent::Send { mime_type, fd } => {
            if let Some(outbound) = &state.dnd.outbound {
                let bytes: Arc<[u8]> = outbound_bytes(&outbound.data, &mime_type).into();
                io::serve_in_background(fd, bytes, format!("a drag as {mime_type}"));
            }
        }
        DataSourceEvent::Action {
            dnd_action: WEnum::Value(action),
        } => {
            if let Some(outbound) = &mut state.dnd.outbound {
                outbound.action = action;
            }
        }
        DataSourceEvent::DndDropPerformed => {
            if let Some(outbound) = &mut state.dnd.outbound {
                outbound.dropped = true;
            }
        }
        // The destination finished reading: success.
        DataSourceEvent::DndFinished => {
            let action = state
                .dnd
                .outbound
                .as_ref()
                .map_or(DndAction::empty(), |o| o.action);
            finish_outbound(state, outcome_of(action));
        }
        // Before a drop: rejected or aborted. After one: some compositors end
        // with `cancelled` rather than `dnd_finished` once the target has the
        // data, so it still reports success, or no terminal event would ever
        // fire.
        DataSourceEvent::Cancelled => {
            let (dropped, action) = state
                .dnd
                .outbound
                .as_ref()
                .map_or((false, DndAction::empty()), |o| (o.dropped, o.action));
            let outcome = if dropped {
                outcome_of(action)
            } else {
                DropOutcome::Cancelled
            };
            finish_outbound(state, outcome);
        }
        _ => {}
    }
}

/// A drag event on the device (`selection` and `data_offer` are the hub's).
pub(super) fn device_event(state: &mut HubState, event: DataDeviceEvent) {
    match event {
        DataDeviceEvent::Enter {
            serial,
            surface,
            x,
            y,
            id,
        } => {
            // A drag replacing one we never saw leave: let its offer go.
            if let Some(previous) = state.dnd.offer.take()
                && id.as_ref() != Some(&previous)
            {
                previous.destroy();
            }
            state.dnd.offer = id.clone();
            let Some(route) = state.dnd.routes.get(&surface.id().protocol_id()).cloned() else {
                // Not one of our surfaces (one already detached).
                state.dnd.active_route = None;
                return;
            };
            let mimes = id.as_ref().map(offer_mimes).unwrap_or_default();
            state.dnd.active_route = Some(route);
            state.dnd.position = Point::new(x as f32, y as f32);
            // Accept a type we can read AND negotiate an action — both, or the
            // compositor shows the "forbidden" cursor and blocks the drop.
            state.dnd.offer_serial = serial;
            state.dnd.offer_accepted_mime = pick_mime(&mimes);
            if let Some(offer) = &id {
                if let Some(mime) = state.dnd.offer_accepted_mime.clone() {
                    offer.accept(serial, Some(mime));
                }
                if offer.version() >= 3 {
                    offer.set_actions(DndAction::Copy, DndAction::Copy);
                }
            }
            // The bytes come at the drop; the types let the target decide
            // on hover.
            state.dnd.post_routed(ExternalDragEvent::Entered {
                data: ExternalDropData {
                    formats: mimes,
                    ..Default::default()
                },
                position: state.dnd.position,
            });
        }
        DataDeviceEvent::Motion { x, y, .. } => {
            if state.dnd.active_route.is_none() {
                return;
            }
            state.dnd.position = Point::new(x as f32, y as f32);
            state.dnd.post_routed(ExternalDragEvent::Moved {
                position: state.dnd.position,
            });
        }
        DataDeviceEvent::Leave => {
            state.dnd.post_routed(ExternalDragEvent::Left);
            state.dnd.active_route = None;
            if let Some(offer) = state.dnd.offer.take() {
                offer.destroy();
            }
            state.dnd.offer_accepted_mime = None;
            state.dnd.offer_serial = 0;
        }
        DataDeviceEvent::Drop => {
            let Some(route) = state.dnd.active_route.take() else {
                return;
            };
            let position = state.dnd.position;
            state.dnd.offer_accepted_mime = None;
            state.dnd.offer_serial = 0;
            let offer = state.dnd.offer.take();
            // Our own drag dropping on our own window: the framework recovers
            // the typed payload from its stash, and the bytes would have to
            // come from our own source, so they are not read.
            if state.dnd.outbound.is_some() {
                route.post(ExternalDragEvent::Dropped {
                    data: ExternalDropData::default(),
                    position,
                });
                if let Some(offer) = offer {
                    finish_offer(&offer);
                }
                return;
            }
            let Some(offer) = offer else {
                route.post(ExternalDragEvent::Dropped {
                    data: ExternalDropData::default(),
                    position,
                });
                return;
            };
            receive_drop(state.conn().clone(), offer, route, position);
        }
        _ => {}
    }
}

/// `finish` (version 3) tells the source the transfer is complete; `destroy`
/// is always due.
fn finish_offer(offer: &WlDataOffer) {
    if offer.version() >= 3 {
        offer.finish();
    }
    offer.destroy();
}

/// Read the drop's payload on a thread of its own, then post it and finish the
/// offer. The source is another client, and reading it on the hub's thread
/// would hold up the clipboard for as long as that client takes.
fn receive_drop(
    conn: wayland_client::Connection,
    offer: WlDataOffer,
    route: Route,
    position: Point,
) {
    let mimes = offer_mimes(&offer);
    let Some(mime) = pick_mime(&mimes) else {
        route.post(ExternalDragEvent::Dropped {
            data: ExternalDropData::default(),
            position,
        });
        finish_offer(&offer);
        return;
    };
    let (read, write) = match io::pipe() {
        Ok(pair) => pair,
        Err(err) => {
            eprintln!("teksilo-platform: cannot read a drop: {err}");
            route.post(ExternalDragEvent::Dropped {
                data: ExternalDropData::default(),
                position,
            });
            finish_offer(&offer);
            return;
        }
    };
    offer.receive(mime.clone(), std::os::fd::AsFd::as_fd(&write));
    let _ = conn.flush();
    drop(write);
    let spawned = std::thread::Builder::new()
        .name("teksilo-wayland-drop".into())
        .spawn(move || {
            let data = match io::read_bounded(read, DROP_LIMITS) {
                Ok(bytes) => decode_drop(&mime, &bytes),
                Err(err) => {
                    eprintln!("teksilo-platform: reading a drop failed: {err}");
                    ExternalDropData::default()
                }
            };
            route.post(ExternalDragEvent::Dropped { data, position });
            finish_offer(&offer);
            let _ = conn.flush();
        });
    if let Err(err) = spawned {
        eprintln!("teksilo-platform: cannot start a thread to read a drop: {err}");
    }
}

/// Pick the best MIME type we can decode from the offered set.
fn pick_mime(offered: &[String]) -> Option<String> {
    PREFERRED_MIMES
        .iter()
        .find_map(|pref| offered.iter().find(|m| m.as_str() == *pref).cloned())
}

fn decode_drop(mime: &str, bytes: &[u8]) -> ExternalDropData {
    let text = String::from_utf8_lossy(bytes).into_owned();
    if mime == "text/uri-list" {
        ExternalDropData::from_uri_list(&text)
    } else {
        ExternalDropData {
            text: Some(text),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_list_is_preferred_over_text() {
        let offered = vec![
            "text/plain".to_string(),
            "text/uri-list".to_string(),
            "text/plain;charset=utf-8".to_string(),
        ];
        assert_eq!(pick_mime(&offered).as_deref(), Some("text/uri-list"));
    }

    #[test]
    fn an_offer_of_nothing_readable_picks_nothing() {
        assert_eq!(pick_mime(&["image/png".to_string()]), None);
    }

    #[test]
    fn a_dropped_uri_list_becomes_files() {
        let data = decode_drop("text/uri-list", b"file:///tmp/a.txt\r\n");
        assert_eq!(data.files.len(), 1);
    }
}
