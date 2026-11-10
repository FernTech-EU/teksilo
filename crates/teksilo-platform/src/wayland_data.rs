// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The one `wl_data_device` this process holds on winit's Wayland connection,
//! shared by the clipboard and external drag-and-drop.
//!
//! # Why one, and why shared
//!
//! The core data-transfer protocol (`wl_data_device_manager`) is the one every
//! compositor implements, GNOME's Mutter included, and the only one a sandbox
//! without an X server can reach. Mutter, however, keeps **one** device per
//! client: `get_data_device` unlinks the client's previous device from its
//! list when the client is not focused, and that device never hears another
//! event (mutter 50, `meta-wayland-data-device.c`, `get_data_device`). A drag
//! `enter` is likewise sent to one device of the client only
//! (`wl_resource_find_for_client`). A clipboard with a device of its own beside
//! the drag-and-drop backend's would therefore silence one or the other at
//! random. So the device is bound once, here, and both subsystems use it.
//!
//! The device belongs to the **seat**, not to a window, which is also what
//! keeps a copied selection alive across windows: its source lives here for as
//! long as the process does, whichever windows open and close.
//!
//! # The connection and the threads
//!
//! The device is bound on **winit's own `wl_display`** through libwayland's
//! multi-queue model ([`Backend::from_foreign_display`]), with an event queue of
//! our own. The connection has to be winit's: the compositor gives the
//! selection only to the client with keyboard focus, and focus belongs to the
//! client whose surface the user clicked, which is winit's.
//!
//! A thread of ours services that queue. winit cannot: it only calls back for
//! events on its own queue, and skips its `about_to_wait` on a wake-up that
//! dispatched nothing of its own (`winit` 0.30, Wayland `pump_events`, "Reduce
//! spurious wake-ups"). Yet a `wl_data_source::send` (another application
//! pasting what we copied) arrives exactly when we are idle and unfocused. The
//! thread takes part in reading the socket through libwayland's
//! `prepare_read` / `read_events` protocol, the one Mesa's EGL uses on an
//! application's display from its own threads: a prepared reader is woken by
//! the same readiness as winit's, and libwayland lets exactly one of them
//! read, queuing every event on its own queue. Nothing is missed and nothing
//! is read twice.
//!
//! The UI thread reaches the state through a mutex and **dispatches the queue
//! itself first** ([`WaylandDataHub::with_state`]): when winit runs a Copy
//! command for a key press, our copy of that key event is already queued (the
//! same read delivered both), so dispatching it before choosing a serial is
//! what makes the serial current (see [`serials`]).
//!
//! The hub keeps the display connection alive (`keepalive`) for as long as it
//! exists, so its proxies never outlive the display they were created on, and
//! a process-wide registry hands every caller on the same display the same hub.

pub(crate) mod dnd;
pub(crate) mod io;
pub(crate) mod serials;

use std::any::Any;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::sync::{Arc, Mutex, OnceLock};

use wayland_backend::client::WaylandError;
use wayland_backend::sys::client::Backend;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_callback::{Event as CallbackEvent, WlCallback};
use wayland_client::protocol::wl_data_device::{Event as DataDeviceEvent, WlDataDevice};
use wayland_client::protocol::wl_data_device_manager::WlDataDeviceManager;
use wayland_client::protocol::wl_data_offer::{Event as DataOfferEvent, WlDataOffer};
use wayland_client::protocol::wl_data_source::{Event as DataSourceEvent, WlDataSource};
use wayland_client::protocol::wl_keyboard::{Event as KeyboardEvent, KeyState, WlKeyboard};
use wayland_client::protocol::wl_pointer::{ButtonState, Event as PointerEvent, WlPointer};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::{Capability, Event as SeatEvent, WlSeat};
use wayland_client::protocol::wl_touch::{Event as TouchEvent, WlTouch};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};

use crate::external_dnd::TouchSerialSource;
use serials::InputSerials;

/// The MIME types an offer carries, filled by its `offer` events. Shared so a
/// clone of the offer proxy reads the same list.
pub(crate) type OfferMimes = Arc<Mutex<Vec<String>>>;

/// Snapshot of an offer's MIME types.
pub(crate) fn offer_mimes(offer: &WlDataOffer) -> Vec<String> {
    offer
        .data::<OfferMimes>()
        .and_then(|mimes| mimes.lock().ok().map(|m| m.clone()))
        .unwrap_or_default()
}

/// What a `wl_data_source` of ours is for.
#[cfg_attr(
    not(feature = "clipboard"),
    allow(
        dead_code,
        reason = "the clipboard half of the hub, used when the `clipboard` feature is on"
    )
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceKind {
    /// The clipboard selection.
    Selection,
    /// An outbound drag.
    Drag,
}

/// Bytes per MIME type, as a source of ours serves them.
#[cfg_attr(
    not(feature = "clipboard"),
    allow(
        dead_code,
        reason = "the clipboard half of the hub, used when the `clipboard` feature is on"
    )
)]
pub(crate) type Payload = Vec<(String, Arc<[u8]>)>;

/// The selection this process owns: the source the compositor holds as the
/// clipboard, what it serves, and an opaque value its owner keeps alongside
/// (the clipboard keeps the text and HTML it was given, so a paste in this
/// process returns exactly that rather than a re-decoding of the bytes).
#[cfg_attr(
    not(feature = "clipboard"),
    allow(
        dead_code,
        reason = "the clipboard half of the hub, used when the `clipboard` feature is on"
    )
)]
struct OwnSelection {
    source: WlDataSource,
    payload: Payload,
    owner_data: Arc<dyn Any + Send + Sync>,
}

/// A `set_selection` the compositor has not answered yet. A `wl_display.sync`
/// sent right after the request comes back once the compositor has handled
/// it; by then an accepted source has been announced back to us (`selection`)
/// and the source it replaced has been cancelled.
#[cfg_attr(
    not(feature = "clipboard"),
    allow(
        dead_code,
        reason = "the clipboard half of the hub, used when the `clipboard` feature is on"
    )
)]
struct SelectionCheck {
    source: WlDataSource,
    /// The source of ours that was the selection when the request went out.
    previous: Option<WlDataSource>,
    /// Whether a `selection` event arrived between the sync of the request
    /// before this one and this one's.
    announced: bool,
}

/// Sources of ours kept while the compositor has not cancelled them. A
/// compositor cancels a replaced source as it takes the new one, so only a
/// non-conforming one lets this many build up; the oldest then goes.
#[cfg_attr(
    not(feature = "clipboard"),
    allow(
        dead_code,
        reason = "the clipboard half of the hub, used when the `clipboard` feature is on"
    )
)]
const MAX_OWNED_SOURCES: usize = 8;

/// The selection as this process sees it.
#[cfg_attr(
    not(feature = "clipboard"),
    allow(
        dead_code,
        reason = "the clipboard half of the hub, used when the `clipboard` feature is on"
    )
)]
pub(crate) enum SelectionView {
    /// Ours: what we set, as we set it.
    Own(Arc<dyn Any + Send + Sync>),
    /// Another client's, with the MIME types it offers.
    Foreign(Vec<String>),
    /// Nothing on the clipboard, or nothing we were told about (the selection
    /// reaches a client only while it has keyboard focus).
    Empty,
}

/// State touched by the dispatch handlers and, under the hub's mutex, by the
/// UI thread.
pub(crate) struct HubState {
    conn: Connection,
    qh: QueueHandle<HubState>,
    manager: WlDataDeviceManager,
    device: WlDataDevice,
    _seat: WlSeat,
    /// Bound on the seat's announcement only, never ahead of it. Mutter
    /// served a `get_touch` on a seat without touch out of an uninitialised
    /// list and gnome-shell crashed, taking the whole session with it; the same
    /// rule is followed for every device here.
    keyboard: Option<WlKeyboard>,
    pointer: Option<WlPointer>,
    touch: Option<WlTouch>,
    /// Input serial and keyboard focus, for `set_selection`.
    serials: InputSerials,
    /// Press serials per device class, for `start_drag`.
    pub(crate) press_serials: TouchSerialSource,
    /// The compositor's current selection, as last announced to us.
    selection_offer: Option<WlDataOffer>,
    /// The selections we set that the compositor has not cancelled, oldest
    /// first. The last is the one we believe current. An older one stays until
    /// its `cancelled`: when the compositor refuses a newer copy, the older one
    /// is still the selection and still has to be served.
    owned: Vec<OwnSelection>,
    /// `set_selection` requests waiting for their `wl_display.sync`.
    checks: Vec<SelectionCheck>,
    /// External drag-and-drop.
    pub(crate) dnd: dnd::DndState,
}

impl HubState {
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }

    /// The clipboard as this process sees it.
    #[cfg_attr(
        not(feature = "clipboard"),
        allow(
            dead_code,
            reason = "the clipboard half of the hub, used when the `clipboard` feature is on"
        )
    )]
    pub(crate) fn selection(&self) -> SelectionView {
        if let Some(own) = self.owned.last() {
            return SelectionView::Own(own.owner_data.clone());
        }
        match &self.selection_offer {
            Some(offer) => SelectionView::Foreign(offer_mimes(offer)),
            None => SelectionView::Empty,
        }
    }

    /// Whether one of our surfaces has keyboard focus.
    #[cfg(all(test, feature = "clipboard"))]
    pub(crate) fn has_keyboard_focus(&self) -> bool {
        self.serials.has_keyboard_focus()
    }

    /// Take the clipboard: offer `payload` as the new selection.
    ///
    /// The compositor answers later, and may refuse without a word: Mutter
    /// ignores a serial no newer than the one the current selection was set
    /// with, two copies with no input between them included. So the source
    /// this replaces is not destroyed here. It goes when the compositor
    /// cancels it, which it does as it takes the new one; if the new one is
    /// refused instead, the older one is still the clipboard, and still ours
    /// to serve (see [`Self::selection_checked`]).
    #[cfg_attr(
        not(feature = "clipboard"),
        allow(
            dead_code,
            reason = "the clipboard half of the hub, used when the `clipboard` feature is on"
        )
    )]
    pub(crate) fn set_selection(
        &mut self,
        payload: Payload,
        owner_data: Arc<dyn Any + Send + Sync>,
    ) -> Result<(), String> {
        let serial = self
            .serials
            .for_selection()
            .map_err(|err| format!("the Wayland clipboard cannot be set: {err}"))?;
        let source = self
            .manager
            .create_data_source(&self.qh, SourceKind::Selection);
        for (mime, _) in &payload {
            source.offer(mime.clone());
        }
        self.device.set_selection(Some(&source), serial);
        self.conn.display().sync(&self.qh, source.clone());
        self.checks.push(SelectionCheck {
            source: source.clone(),
            previous: self.owned.last().map(|own| own.source.clone()),
            announced: false,
        });
        if self.owned.len() == MAX_OWNED_SOURCES {
            self.owned.remove(0).source.destroy();
        }
        self.owned.push(OwnSelection {
            source,
            payload,
            owner_data,
        });
        Ok(())
    }

    /// The compositor has handled the `set_selection` of `source`. It refused
    /// it when it neither announced a new selection to us nor cancelled the
    /// source of ours it would have replaced: that one is still the clipboard.
    /// The refused source is dropped, so what this process reads back is what
    /// every other application reads.
    ///
    /// With no earlier source of ours there is nothing to tell a silent refusal
    /// by, and the new source is kept; a refusal with a cancel (Mutter's for a
    /// client without keyboard focus) is handled by the `cancelled` itself.
    fn selection_checked(&mut self, source: &WlDataSource) {
        let Some(at) = self.checks.iter().position(|c| &c.source == source) else {
            return;
        };
        let check = self.checks.remove(at);
        let previous_kept = check
            .previous
            .as_ref()
            .is_some_and(|previous| self.owned.iter().any(|own| &own.source == previous));
        if check.announced || !previous_kept {
            return;
        }
        if let Some(at) = self.owned.iter().position(|own| own.source == check.source) {
            self.owned.remove(at);
            check.source.destroy();
            eprintln!(
                "teksilo-platform: the compositor ignored a copy (no input since the previous one?); the clipboard keeps what was copied before"
            );
        }
    }

    /// Ask the selection's owner to write `mime` into a pipe, and return the
    /// pipe's read end. The caller reads it with [`io::read_bounded`] after
    /// releasing the hub, so the hub's thread stays free meanwhile.
    #[cfg_attr(
        not(feature = "clipboard"),
        allow(
            dead_code,
            reason = "the clipboard half of the hub, used when the `clipboard` feature is on"
        )
    )]
    pub(crate) fn receive_selection(&mut self, mime: &str) -> Result<OwnedFd, String> {
        let offer = self
            .selection_offer
            .as_ref()
            .ok_or_else(|| "the clipboard is empty".to_string())?;
        let (read, write) = io::pipe().map_err(|err| format!("cannot create a pipe: {err}"))?;
        offer.receive(mime.to_string(), write.as_fd());
        match self.conn.flush() {
            Ok(()) => {}
            // A full socket buffer: libwayland keeps the request, with its own
            // copy of the descriptor, and the next flush sends it. The bounded
            // read covers the wait.
            Err(WaylandError::Io(io)) if io.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(err) => return Err(format!("cannot reach the compositor: {err}")),
        }
        // Our copy of the write end is closed here, so the reader sees the end
        // once the owner closes theirs.
        drop(write);
        Ok(read)
    }

    /// Serve `mime` of the selection we own over `fd`.
    fn serve_selection(&self, source: &WlDataSource, mime: &str, fd: OwnedFd) {
        let Some(own) = self.owned.iter().find(|own| &own.source == source) else {
            // A source we no longer keep: it has nothing to say any more.
            // Closing the pipe gives the reader an empty answer at once.
            return;
        };
        let Some((_, bytes)) = own.payload.iter().find(|(m, _)| m == mime) else {
            return;
        };
        io::serve_in_background(fd, bytes.clone(), format!("the clipboard as {mime}"));
    }
}

struct HubInner {
    queue: EventQueue<HubState>,
    state: HubState,
}

/// See the module documentation.
pub(crate) struct WaylandDataHub {
    conn: Connection,
    inner: Mutex<HubInner>,
    /// Address of the `wl_display` this hub serves, its registry key.
    display: usize,
    /// Written to stop the thread.
    #[cfg_attr(
        not(all(test, feature = "clipboard")),
        allow(dead_code, reason = "only the compositor tests stop a hub")
    )]
    stop_write: OwnedFd,
    /// Keeps the display connection open while the hub exists: winit's owned
    /// display handle in an application.
    _keepalive: Box<dyn Any + Send + Sync>,
}

/// Every hub of the process, one per display (an application has one).
fn registry() -> &'static Mutex<Vec<Arc<WaylandDataHub>>> {
    static HUBS: OnceLock<Mutex<Vec<Arc<WaylandDataHub>>>> = OnceLock::new();
    HUBS.get_or_init(|| Mutex::new(Vec::new()))
}

impl WaylandDataHub {
    /// The hub for winit's Wayland display, created on first use.
    ///
    /// `Err` when `display` is not a Wayland display, or when the compositor
    /// offers no seat or no data device manager.
    pub(crate) fn for_display(
        display: &winit::event_loop::OwnedDisplayHandle,
    ) -> Result<Arc<Self>, String> {
        use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
        let raw = display
            .display_handle()
            .map_err(|err| format!("no display handle: {err}"))?
            .as_raw();
        let RawDisplayHandle::Wayland(wayland) = raw else {
            return Err("the display is not a Wayland display".into());
        };
        let ptr = wayland.display.as_ptr();
        let mut hubs = registry()
            .lock()
            .map_err(|_| "the Wayland data registry is poisoned".to_string())?;
        if let Some(hub) = hubs.iter().find(|hub| hub.display == ptr as usize) {
            return Ok(hub.clone());
        }
        // SAFETY: `ptr` is the live `wl_display` of winit's connection, which
        // `keepalive` (a clone of winit's owned display handle) keeps open for
        // as long as the hub, and so every proxy made on it, exists.
        let backend = unsafe { Backend::from_foreign_display(ptr.cast()) };
        let conn = Connection::from_backend(backend);
        let hub = Self::start(conn, ptr as usize, Box::new(display.clone()))?;
        hubs.push(hub.clone());
        Ok(hub)
    }

    /// The hub already serving the display at `ptr`, if one was created by
    /// [`Self::for_display`].
    pub(crate) fn existing(ptr: usize) -> Option<Arc<Self>> {
        registry()
            .lock()
            .ok()?
            .iter()
            .find(|hub| hub.display == ptr)
            .cloned()
    }

    /// A hub on a connection of the caller's own, kept out of the registry.
    /// For a client that is not winit (the compositor tests).
    #[cfg(all(test, feature = "clipboard"))]
    pub(crate) fn for_connection(conn: Connection) -> Result<Arc<Self>, String> {
        let display = conn.backend().display_ptr() as usize;
        Self::start(conn.clone(), display, Box::new(conn))
    }

    fn start(
        conn: Connection,
        display: usize,
        keepalive: Box<dyn Any + Send + Sync>,
    ) -> Result<Arc<Self>, String> {
        let (globals, mut queue) = registry_queue_init::<HubState>(&conn)
            .map_err(|err| format!("cannot read the compositor's globals: {err}"))?;
        let qh = queue.handle();
        let seat = globals
            .bind::<WlSeat, _, _>(&qh, 1..=5, ())
            .map_err(|err| format!("the compositor offers no seat: {err}"))?;
        let manager = globals
            .bind::<WlDataDeviceManager, _, _>(&qh, 1..=3, ())
            .map_err(|err| format!("the compositor offers no data device manager: {err}"))?;
        let device = manager.get_data_device(&seat, &qh, ());
        let mut state = HubState {
            conn: conn.clone(),
            qh,
            manager,
            device,
            _seat: seat,
            keyboard: None,
            pointer: None,
            touch: None,
            serials: InputSerials::default(),
            press_serials: TouchSerialSource::default(),
            selection_offer: None,
            owned: Vec::new(),
            checks: Vec::new(),
            dnd: dnd::DndState::default(),
        };
        // The seat's capabilities, and with them the keyboard whose focus the
        // first copy needs, before anyone asks.
        queue
            .roundtrip(&mut state)
            .map_err(|err| format!("the compositor did not answer: {err}"))?;
        let (stop_read, stop_write) =
            io::pipe().map_err(|err| format!("cannot create a pipe: {err}"))?;
        let hub = Arc::new(Self {
            conn,
            inner: Mutex::new(HubInner { queue, state }),
            display,
            stop_write,
            _keepalive: keepalive,
        });
        let for_thread = hub.clone();
        std::thread::Builder::new()
            .name("teksilo-wayland-data".into())
            .spawn(move || for_thread.run(stop_read))
            .map_err(|err| format!("cannot start the Wayland data thread: {err}"))?;
        Ok(hub)
    }

    /// Dispatch what is queued for us, then run `f` on the state, then send
    /// what `f` asked for. Any thread; the UI thread in practice.
    pub(crate) fn with_state<R>(&self, f: impl FnOnce(&mut HubState) -> R) -> Result<R, String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "the Wayland data state is poisoned".to_string())?;
        let HubInner { queue, state } = &mut *inner;
        queue
            .dispatch_pending(state)
            .map_err(|err| format!("the Wayland connection failed: {err}"))?;
        let result = f(state);
        // A full socket buffer (`WouldBlock`) is flushed by the next writer;
        // anything else means the connection is gone, which the next dispatch
        // reports.
        let _ = self.conn.flush();
        Ok(result)
    }

    /// Stop the dispatch thread. Pending events are left unread. For the
    /// compositor tests, which open and close several connections; an
    /// application's hub lives as long as the process.
    #[cfg(all(test, feature = "clipboard"))]
    pub(crate) fn shutdown(&self) {
        let one = [1u8];
        // SAFETY: `one` is a valid one-byte buffer; `stop_write` is open.
        let _ = unsafe { libc::write(self.stop_write.as_raw_fd(), one.as_ptr().cast(), 1) };
    }

    /// The dispatch thread: dispatch, prepare to read, wait for the socket or
    /// the stop pipe, read, and again.
    fn run(self: Arc<Self>, stop_read: OwnedFd) {
        loop {
            let guard = {
                let Ok(mut inner) = self.inner.lock() else {
                    eprintln!(
                        "teksilo-platform: the Wayland data state is poisoned; clipboard and drag-and-drop stop"
                    );
                    return;
                };
                let HubInner { queue, state } = &mut *inner;
                if let Err(err) = queue.dispatch_pending(state) {
                    eprintln!(
                        "teksilo-platform: the Wayland connection failed ({err}); clipboard and drag-and-drop stop"
                    );
                    return;
                }
                let _ = self.conn.flush();
                queue.prepare_read()
            };
            // `None`: events arrived for us between the dispatch and the
            // prepare. Dispatch them first.
            let Some(guard) = guard else { continue };
            let mut fds = [
                libc::pollfd {
                    fd: guard.connection_fd().as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: stop_read.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            // SAFETY: `fds` holds two valid pollfds for the call's duration.
            let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
            if ready < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    // Dropping the guard cancels the read; prepare again.
                    continue;
                }
                eprintln!(
                    "teksilo-platform: waiting on the Wayland connection failed ({}); clipboard and drag-and-drop stop",
                    std::io::Error::last_os_error()
                );
                return;
            }
            if fds[1].revents != 0 {
                // Stopping: the guard's drop cancels our part of the read, so
                // no other reader is left waiting on us.
                return;
            }
            if let Err(err) = guard.read() {
                match err {
                    wayland_backend::client::WaylandError::Io(io)
                        if io.kind() == std::io::ErrorKind::WouldBlock => {}
                    other => {
                        eprintln!(
                            "teksilo-platform: reading the Wayland connection failed ({other}); clipboard and drag-and-drop stop"
                        );
                        return;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

// The registry is driven by `GlobalList`; this impl is just the required bound.
impl Dispatch<WlRegistry, GlobalListContents> for HubState {
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

impl Dispatch<WlDataDeviceManager, ()> for HubState {
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

impl Dispatch<WlSeat, ()> for HubState {
    fn event(
        state: &mut Self,
        seat: &WlSeat,
        event: SeatEvent,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        // `get_keyboard` / `get_pointer` / `get_touch` may only be issued for
        // a capability the seat has announced: the protocol makes asking
        // without one the client's error, and Mutter answered an early
        // `get_touch` by crashing the session (see `HubState::touch`).
        let SeatEvent::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        else {
            return;
        };

        if capabilities.contains(Capability::Keyboard) {
            if state.keyboard.is_none() {
                state.keyboard = Some(seat.get_keyboard(qh, ()));
                state.serials.set_keyboard_bound(true);
            }
        } else if let Some(keyboard) = state.keyboard.take() {
            if keyboard.version() >= 3 {
                keyboard.release();
            }
            state.serials.set_keyboard_bound(false);
        }

        if capabilities.contains(Capability::Pointer) {
            if state.pointer.is_none() {
                state.pointer = Some(seat.get_pointer(qh, ()));
            }
        } else if let Some(pointer) = state.pointer.take()
            && pointer.version() >= 3
        {
            pointer.release();
        }

        if capabilities.contains(Capability::Touch) {
            if state.touch.is_none() {
                state.touch = Some(seat.get_touch(qh, ()));
            }
        } else if let Some(touch) = state.touch.take()
            && touch.version() >= 3
        {
            touch.release();
        }
    }
}

impl Dispatch<WlKeyboard, ()> for HubState {
    fn event(
        state: &mut Self,
        _: &WlKeyboard,
        event: KeyboardEvent,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            KeyboardEvent::Enter { serial, .. } => state.serials.keyboard_enter(serial),
            KeyboardEvent::Leave { .. } => state.serials.keyboard_leave(),
            KeyboardEvent::Key {
                serial,
                state: key_state,
                ..
            } => {
                // A press or a release: both are input the user gave us.
                if matches!(
                    key_state,
                    WEnum::Value(KeyState::Pressed | KeyState::Released)
                ) {
                    state.serials.record(serial);
                }
            }
            // The keymap's descriptor is closed as the event drops.
            _ => {}
        }
    }
}

impl Dispatch<WlPointer, ()> for HubState {
    fn event(
        state: &mut Self,
        _: &WlPointer,
        event: PointerEvent,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            PointerEvent::Button {
                serial, state: btn, ..
            } => {
                state.serials.record(serial);
                // The press that opens an implicit grab is the serial
                // `start_drag` must be handed.
                if btn == WEnum::Value(ButtonState::Pressed) {
                    state.press_serials.record_pointer(serial);
                } else {
                    state.press_serials.record_pointer_release();
                    dnd::release_reached_us(state, false);
                }
            }
            // A button held across a leave (a drag grab took it) is released
            // where we cannot see it.
            PointerEvent::Enter { .. } | PointerEvent::Leave { .. } => {
                state.press_serials.record_pointer_focus_change();
            }
            _ => {}
        }
    }
}

impl Dispatch<WlTouch, ()> for HubState {
    fn event(
        state: &mut Self,
        _: &WlTouch,
        event: TouchEvent,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            TouchEvent::Down { serial, .. } => {
                state.serials.record(serial);
                state.press_serials.record_touch(serial);
            }
            TouchEvent::Up { .. } => {
                state.press_serials.record_touch_up();
                dnd::release_reached_us(state, true);
            }
            // The compositor took every touch point away: none is held.
            TouchEvent::Cancel => state.press_serials.record_touch_cancel(),
            _ => {}
        }
    }
}

impl Dispatch<WlDataOffer, OfferMimes> for HubState {
    fn event(
        _: &mut Self,
        _: &WlDataOffer,
        event: DataOfferEvent,
        mimes: &OfferMimes,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let DataOfferEvent::Offer { mime_type } = event
            && let Ok(mut mimes) = mimes.lock()
        {
            mimes.push(mime_type);
        }
    }
}

impl Dispatch<WlDataSource, SourceKind> for HubState {
    fn event(
        state: &mut Self,
        source: &WlDataSource,
        event: DataSourceEvent,
        kind: &SourceKind,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match kind {
            SourceKind::Selection => match event {
                DataSourceEvent::Send { mime_type, fd } => {
                    state.serve_selection(source, &mime_type, fd);
                }
                DataSourceEvent::Cancelled => {
                    // Replaced (by another client's copy or a newer one of
                    // ours), or refused. Either way it is no longer the
                    // clipboard; an older source of ours, if one is left, still
                    // may be.
                    state.owned.retain(|own| &own.source != source);
                    source.destroy();
                }
                _ => {}
            },
            SourceKind::Drag => dnd::source_event(state, source, event),
        }
    }
}

/// The `wl_display.sync` sent after a `set_selection`, carrying its source.
impl Dispatch<WlCallback, WlDataSource> for HubState {
    fn event(
        state: &mut Self,
        _: &WlCallback,
        event: CallbackEvent,
        source: &WlDataSource,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let CallbackEvent::Done { .. } = event {
            state.selection_checked(source);
        }
    }
}

impl Dispatch<WlDataDevice, ()> for HubState {
    fn event(
        state: &mut Self,
        _: &WlDataDevice,
        event: DataDeviceEvent,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            // Introduces an offer; its MIME types follow on the offer itself,
            // and a `selection` or an `enter` then says what it is for.
            DataDeviceEvent::DataOffer { .. } => {}
            DataDeviceEvent::Selection { id } => {
                // The compositor handles requests in order, so this answers the
                // oldest `set_selection` still waiting: the others' syncs come
                // back after its own.
                if let Some(oldest) = state.checks.first_mut() {
                    oldest.announced = true;
                }
                if let Some(previous) = state.selection_offer.take()
                    && id.as_ref() != Some(&previous)
                {
                    previous.destroy();
                }
                state.selection_offer = id;
            }
            other => dnd::device_event(state, other),
        }
    }

    // The `data_offer` event creates a `wl_data_offer`; tell wayland-client its
    // interface and user data, or it panics on the first one.
    wayland_client::event_created_child!(HubState, WlDataDevice, [
        wayland_client::protocol::wl_data_device::EVT_DATA_OFFER_OPCODE => (WlDataOffer, OfferMimes::default()),
    ]);
}
