// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! X11 external drag-and-drop backend (XDND v5), both directions.
//!
//! # Why a proxy window (the inbound half)
//!
//! XDND messages are `ClientMessage`s sent with an **empty event mask**, which
//! the X protocol delivers only to the client that *created* the destination
//! window. winit created the toplevel and pumps its own connection, so a second
//! connection simply cannot see them — and winit's public API exposes no hook
//! into its X event stream (`WindowExtX11` is an empty trait) and no way to
//! disable its own built-in XDND handling.
//!
//! The XDND spec's own answer is `XdndProxy`: a window may name another window
//! that "should be checked for `XdndAware` and should receive all the client
//! messages". So we create a 1×1 `InputOnly` helper window on **our**
//! connection, mark it `XdndAware` + self-pointing `XdndProxy` (the spec's
//! stale-proxy guard), and set `XdndProxy` on winit's toplevel to point at it.
//! Proxy-aware sources then talk to us directly, with full position and
//! arbitrary MIME types.
//!
//! GTK 3/4 (`gdkdnd-x11.c::xdnd_check_dest`), Qt 5/6 (`qxcbdrag.cpp::xdndProxy`)
//! and Java/AWT all honour `XdndProxy` with the self-pointing validation, which
//! covers every mainstream toolkit and file manager. A source that ignores it
//! reaches winit instead, whose built-in XDND handling Teksilo does not
//! consume — such a drop is ignored rather than delivered. No mainstream
//! toolkit is affected; see `docs/drag-and-drop.md` §11.3.1.
//!
//! # Why no pointer grab (the outbound half)
//!
//! An XDND source conventionally grabs the pointer to keep receiving motion
//! once the cursor is over another application's window. We can't: X11 pointer
//! grabs are exclusive per client, and the `ButtonPress` that started the drag
//! already gave winit's connection an implicit grab that lasts until the button
//! is released — so `GrabPointer` from here would return `AlreadyGrabbed` every
//! single time, not just occasionally.
//!
//! We don't need one. `QueryPointer` is unaffected by grabs and reports both
//! the pointer position *and* the button mask, so the drag is driven by polling
//! our own connection while the button is held. That also keeps the outbound
//! path independent of winit's event stream entirely.
//!
//! # Threading
//!
//! One thread and one `RustConnection` per window. X11 has no equivalent of
//! libwayland's multi-queue model, so sharing winit's connection would race on
//! sequence numbers; ours is entirely separate. The thread blocks in `poll(2)`
//! on the X socket plus a self-pipe, so it costs nothing while idle and wakes
//! immediately on teardown. Every event reaches the widget tree through
//! `AppEventPoster::post_external`, never by touching the tree directly.

use std::os::fd::{AsRawFd, RawFd};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use raw_window_handle::RawWindowHandle;
use teksilo_canvas::Point;
use teksilo_core::raw_handle::ParentHandle;
use teksilo_core::window::TeksiloWindowId;
use teksilo_core::{
    AppEventPoster, DragImageData, DropOutcome, ExternalDropData, OutboundDragData,
};
use x11rb::connection::Connection as _;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, KeyButMask, Property,
    SelectionNotifyEvent, Timestamp, Window, WindowClass,
};

use crate::x11::connection::{X11Connection, X11Error, ignore_errors};
use crate::x11::xdnd;

use super::{
    ExternalDndBackend, ExternalDndEventPayload, ExternalDndGuard, ExternalDragEvent, NoopDndGuard,
    outbound_bytes, outbound_mimes,
};

/// How often the outbound source samples the pointer while a drag is in
/// flight. 8 ms tracks a 120 Hz pointer without flooding the server; between
/// drags the thread blocks indefinitely and costs nothing.
const OUTBOUND_POLL: Duration = Duration::from_millis(8);

/// How long to wait for a target's `XdndFinished` after we send `XdndDrop`.
///
/// A target that dies mid-transfer, or one that simply never answers, must not
/// strand the source widget without its terminal `on_drag_ended`. Qt waits
/// minutes here; a few seconds is friendlier and still far longer than any
/// real handshake.
const FINISH_TIMEOUT: Duration = Duration::from_secs(5);

/// Above this, a selection is served with the ICCCM `INCR` protocol instead of
/// one property write. Well under the 256 KiB floor the protocol guarantees for
/// `maximum-request-length`, so it holds on any server.
const INCR_THRESHOLD: usize = 128 * 1024;

// ============================================================
// Backend + guard
// ============================================================

/// The X11 [`ExternalDndBackend`]. One instance serves the whole app; each
/// window gets its own thread, connection, and proxy window.
#[derive(Default)]
pub struct X11ExternalDndBackend;

impl X11ExternalDndBackend {
    pub fn new() -> Self {
        Self
    }
}

impl ExternalDndBackend for X11ExternalDndBackend {
    fn attach(
        &mut self,
        parent: ParentHandle,
        window_id: TeksiloWindowId,
        poster: Arc<dyn AppEventPoster>,
    ) -> Box<dyn ExternalDndGuard> {
        let Some(toplevel) = x11_window_id(&parent) else {
            // Not an X11 window (this backend was selected in error, or the
            // handle shape changed): stay inert rather than guessing.
            return Box::new(NoopDndGuard);
        };

        let shutdown = Arc::new(AtomicBool::new(false));
        // f64 bits; 1.0 until the app pushes the real scale (it does so
        // immediately after attach, but a drop could in principle land first).
        let scale = Arc::new(AtomicU64::new(1.0f64.to_bits()));
        let (cmd_tx, cmd_rx) = channel();

        let Ok(waker) = Waker::new() else {
            eprintln!("teksilo-platform: X11 DnD disabled (could not create a wake pipe)");
            return Box::new(NoopDndGuard);
        };
        let wake_read = waker.read_fd();

        let thread_shutdown = shutdown.clone();
        let thread_scale = scale.clone();
        let handle = std::thread::Builder::new()
            .name(format!("teksilo-xdnd-{}", window_id.raw()))
            .spawn(move || {
                if let Err(err) = run_thread(
                    toplevel,
                    window_id,
                    poster,
                    thread_shutdown,
                    thread_scale,
                    wake_read,
                    cmd_rx,
                ) {
                    eprintln!("teksilo-platform: X11 drag-and-drop thread stopped: {err}");
                }
            })
            .ok();

        Box::new(X11DndGuard {
            shutdown,
            scale,
            commands: cmd_tx,
            waker,
            thread: Mutex::new(handle),
        })
    }
}

/// Per-window registration guard. Dropping it tears the thread down and
/// removes the `XdndProxy` pointer from winit's toplevel.
struct X11DndGuard {
    shutdown: Arc<AtomicBool>,
    scale: Arc<AtomicU64>,
    commands: Sender<Command>,
    waker: Waker,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl ExternalDndGuard for X11DndGuard {
    fn begin_drag(&self, data: &OutboundDragData, _image: Option<&DragImageData>) -> bool {
        if data.is_empty() {
            return false;
        }
        // No drag icon window: XDND has no icon in the wire protocol, and the
        // client-side override-redirect window GTK/Qt use for one needs an
        // ARGB visual and a running compositor to avoid drawing a black
        // rectangle. The cursor still changes, which works over other apps'
        // windows because winit's implicit grab is in force.
        if self.commands.send(Command::Begin(data.clone())).is_err() {
            return false;
        }
        self.waker.wake();
        true
    }

    fn cancel_drag(&self) {
        let _ = self.commands.send(Command::Cancel);
        self.waker.wake();
    }

    fn set_scale_factor(&self, scale: f64) {
        if scale.is_finite() && scale > 0.0 {
            self.scale.store(scale.to_bits(), Ordering::Relaxed);
        }
    }
}

impl Drop for X11DndGuard {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        self.waker.wake();
        if let Ok(mut slot) = self.thread.lock()
            && let Some(handle) = slot.take()
        {
            // The thread checks `shutdown` on every loop turn and the wake pipe
            // breaks it out of `poll` immediately, so this returns promptly.
            // Joining (rather than detaching, as the Wayland backend does) is
            // what guarantees the proxy window and `XdndProxy` property are
            // gone before the window itself is destroyed.
            let _ = handle.join();
        }
    }
}

/// Extract the X11 window id from a raw handle, accepting either handle shape
/// winit may produce.
fn x11_window_id(parent: &ParentHandle) -> Option<Window> {
    match parent.raw_window_handle() {
        RawWindowHandle::Xlib(h) => Some(h.window as Window),
        RawWindowHandle::Xcb(h) => Some(h.window.get()),
        _ => None,
    }
}

// ============================================================
// Wake pipe
// ============================================================

/// A self-pipe used to break the DnD thread out of `poll(2)`.
///
/// The alternative — sending ourselves an X message — would need a second
/// connection just for teardown; a pipe is two file descriptors and cannot
/// fail because the X connection is already broken.
struct Waker {
    read: RawFd,
    write: RawFd,
}

impl Waker {
    fn new() -> Result<Self, std::io::Error> {
        let mut fds = [0 as libc::c_int; 2];
        // SAFETY: `fds` is a valid two-element array for `pipe2` to fill.
        // O_CLOEXEC keeps the descriptors out of any child process.
        let rc = unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) };
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self {
            read: fds[0],
            write: fds[1],
        })
    }

    fn read_fd(&self) -> RawFd {
        self.read
    }

    fn wake(&self) {
        let byte = 1u8;
        // SAFETY: `self.write` is an open pipe descriptor owned by `self` and
        // `byte` is a valid one-byte buffer. A full pipe (EAGAIN) is fine — a
        // wakeup is already pending, which is all the reader needs.
        unsafe {
            libc::write(self.write, std::ptr::from_ref(&byte).cast(), 1);
        }
    }
}

impl Drop for Waker {
    fn drop(&mut self) {
        // SAFETY: both descriptors are owned by `self` and closed exactly once.
        unsafe {
            libc::close(self.read);
            libc::close(self.write);
        }
    }
}

/// Block until the X socket or the wake pipe is readable, or `timeout` passes.
/// `None` waits indefinitely.
fn wait_readable(x_fd: RawFd, wake_fd: RawFd, timeout: Option<Duration>) {
    let mut fds = [
        libc::pollfd {
            fd: x_fd,
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: wake_fd,
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    let millis = timeout.map_or(-1, |d| d.as_millis().min(i32::MAX as u128) as libc::c_int);
    // SAFETY: `fds` is a valid array of two initialised pollfds.
    unsafe {
        libc::poll(fds.as_mut_ptr(), 2, millis);
    }
    if fds[1].revents & libc::POLLIN != 0 {
        // Drain the pipe so the next poll blocks again.
        let mut buf = [0u8; 64];
        // SAFETY: `buf` is a valid 64-byte buffer and the fd is non-blocking,
        // so this returns immediately once drained.
        while unsafe { libc::read(wake_fd, buf.as_mut_ptr().cast(), buf.len()) } > 0 {}
    }
}

// ============================================================
// Thread
// ============================================================

/// Work handed from the guard (UI thread) to the DnD thread.
enum Command {
    /// Start exporting this payload as a native OS drag.
    Begin(OutboundDragData),
    /// The user pressed Escape — abandon the outbound drag.
    Cancel,
}

fn run_thread(
    toplevel: Window,
    window_id: TeksiloWindowId,
    poster: Arc<dyn AppEventPoster>,
    shutdown: Arc<AtomicBool>,
    scale: Arc<AtomicU64>,
    wake_fd: RawFd,
    commands: Receiver<Command>,
) -> Result<(), X11Error> {
    let conn = X11Connection::open()?;
    let proxy = create_proxy_window(&conn)?;
    install_proxy(&conn, toplevel, proxy)?;

    let x_fd = conn.conn().stream().as_raw_fd();
    let mut state = DndThread {
        conn,
        poster,
        window_id,
        toplevel,
        proxy,
        scale,
        inbound: None,
        outbound: None,
        incr_sends: Vec::new(),
    };

    // Run the loop to completion, then tear down unconditionally. An early
    // `return` on a connection error would leave `XdndProxy` pointing at a
    // window that no longer answers, and a source that skips the spec's
    // self-pointing validation would send the whole handshake into it.
    let result = pump_until_shutdown(&mut state, &shutdown, &commands, x_fd, wake_fd);
    state.teardown();
    result
}

/// The DnD thread's event loop. Split out so [`run_thread`] can tear down on
/// every exit path.
fn pump_until_shutdown(
    state: &mut DndThread,
    shutdown: &AtomicBool,
    commands: &Receiver<Command>,
    x_fd: RawFd,
    wake_fd: RawFd,
) -> Result<(), X11Error> {
    while !shutdown.load(Ordering::SeqCst) {
        // Drain everything already queued before deciding how long to sleep.
        loop {
            match state.conn.poll_event() {
                Ok(Some(event)) => state.handle_event(event),
                Ok(None) => break,
                // A broken connection is terminal — never retry, or the loop
                // spins at 100% CPU against a dead server.
                Err(err) => return Err(err),
            }
        }

        while let Ok(command) = commands.try_recv() {
            match command {
                Command::Begin(data) => state.begin_outbound(data),
                Command::Cancel => state.cancel_outbound(),
            }
        }

        state.pump_outbound();

        // Poll fast only while a drag is in flight; otherwise sleep until the
        // server or the guard has something to say.
        let timeout = state.outbound.is_some().then_some(OUTBOUND_POLL);
        wait_readable(x_fd, wake_fd, timeout);
    }
    Ok(())
}

/// Create the 1×1 `InputOnly` helper window that fronts XDND for this toplevel.
///
/// `InputOnly` because it must never render — it exists purely as an identity
/// that owns messages, properties, and the selection. It is deliberately left
/// unmapped: sources address it by id (through `XdndProxy`), never by finding
/// it under the pointer.
fn create_proxy_window(conn: &X11Connection) -> Result<Window, X11Error> {
    let proxy = conn.conn().generate_id()?;
    conn.conn()
        .create_window(
            0, // InputOnly windows must have depth 0
            proxy,
            conn.root(),
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0, // CopyFromParent visual
            &CreateWindowAux::new()
                .override_redirect(1)
                // PropertyChange carries INCR chunk notifications and the
                // server-timestamp round trip. ClientMessages need no mask:
                // an empty-mask send is delivered to the creating client.
                .event_mask(EventMask::PROPERTY_CHANGE),
        )?
        .check()?;
    Ok(proxy)
}

/// Publish the proxy: mark it XDND-aware and self-pointing, then point the
/// toplevel at it.
fn install_proxy(conn: &X11Connection, toplevel: Window, proxy: Window) -> Result<(), X11Error> {
    let atoms = conn.atoms();
    conn.set_property32(
        proxy,
        atoms.xdnd_aware,
        AtomEnum::ATOM.into(),
        &[xdnd::XDND_VERSION],
    )?;
    // The spec requires the proxy to name *itself*, so a source can tell a live
    // proxy from a stale property left behind by a crash.
    conn.set_property32(proxy, atoms.xdnd_proxy, AtomEnum::WINDOW.into(), &[proxy])?;
    conn.set_property32(
        toplevel,
        atoms.xdnd_proxy,
        AtomEnum::WINDOW.into(),
        &[proxy],
    )?;
    conn.flush()?;
    Ok(())
}

// ============================================================
// Thread state
// ============================================================

/// An inbound drag currently over this window.
struct Inbound {
    source: Window,
    version: u32,
    /// Every type the source offers, in its order.
    types: Vec<Atom>,
    /// The type we chose to request, if any of ours matched.
    chosen: Option<Atom>,
    /// Latest pointer position, already converted to window-logical.
    position: Point,
    /// True once `Entered` has been posted (which needs a position, so it
    /// happens on the first `XdndPosition`, not on `XdndEnter`).
    entered: bool,
    /// In-flight `INCR` transfer, if the source chose to chunk.
    incr: Option<xdnd::IncrAssembler>,
    /// Set once `XdndDrop` arrives, so a `SelectionNotify` can tell a
    /// drop-time transfer from a speculative one.
    dropping: bool,
}

/// An outbound drag this window started.
struct Outbound {
    data: OutboundDragData,
    /// MIME type ↔ atom for everything we advertise.
    types: Vec<(String, Atom)>,
    /// Timestamp we took ownership of `XdndSelection` with.
    time: Timestamp,
    /// The window the wire protocol names as the target (the one under the
    /// pointer), which is *not* necessarily where messages are sent.
    target: Option<Window>,
    /// Where messages actually go — the target's `XdndProxy`, or the target.
    send_to: Option<Window>,
    /// Negotiated version with the current target.
    version: u32,
    /// Whether the current target says it would accept a drop.
    accepted: bool,
    /// Set after `XdndDrop`; we then wait only for `XdndFinished`.
    dropped_at: Option<Instant>,
    /// Which root-child the cached `target` / `send_to` / `version` were
    /// resolved for. See the cache note in `pump_outbound`.
    resolved_for: Option<Window>,
}

/// One `INCR` transfer we are serving to a requestor.
struct IncrSend {
    requestor: Window,
    property: Atom,
    type_: Atom,
    remaining: Vec<u8>,
    chunk: usize,
}

struct DndThread {
    conn: X11Connection,
    poster: Arc<dyn AppEventPoster>,
    window_id: TeksiloWindowId,
    toplevel: Window,
    proxy: Window,
    scale: Arc<AtomicU64>,
    inbound: Option<Inbound>,
    outbound: Option<Outbound>,
    /// In-flight `INCR` sends, keyed by (requestor, property).
    ///
    /// Owned by the thread rather than by [`Outbound`] because the transfer
    /// routinely outlives the drag: the target sends `XdndFinished` as soon as
    /// it has *requested* the data, and a large payload is still being chunked
    /// out at that point. Parking these inside the drag would drop them the
    /// moment `finish_outbound` takes it, leaving the target waiting on a
    /// `PropertyNotify` that never comes.
    incr_sends: Vec<IncrSend>,
}

impl DndThread {
    fn post(&self, event: ExternalDragEvent) {
        self.poster.post_external(Box::new(ExternalDndEventPayload {
            window_id_owner: self.window_id,
            event,
        }));
    }

    fn scale(&self) -> f64 {
        f64::from_bits(self.scale.load(Ordering::Relaxed))
    }

    /// Convert a root-relative physical point to this window's logical space —
    /// the coordinate contract every `ExternalDragEvent` position uses.
    ///
    /// `None` when the toplevel can no longer be translated against (it is
    /// being destroyed, or has been unmapped). Falling back to the raw root
    /// coordinate would silently report a position in the wrong space — for a
    /// window at root (900, 400) that is a drop reported ~900 px from where it
    /// happened — so the caller skips the event instead.
    fn to_window_logical(&self, root_x: i16, root_y: i16) -> Option<Point> {
        let reply = self
            .conn
            .conn()
            .translate_coordinates(self.conn.root(), self.toplevel, root_x, root_y)
            .ok()?
            .reply()
            .ok()?;
        let scale = self.scale();
        Some(Point::new(
            (reply.dst_x as f64 / scale) as f32,
            (reply.dst_y as f64 / scale) as f32,
        ))
    }

    // ---------- event dispatch ----------

    fn handle_event(&mut self, event: Event) {
        let atoms = self.conn.atoms().clone();
        match event {
            Event::ClientMessage(msg) => {
                let data = msg.data.as_data32();
                let data = [data[0], data[1], data[2], data[3], data[4]];
                match msg.type_ {
                    t if t == atoms.xdnd_enter => self.on_enter(data),
                    t if t == atoms.xdnd_position => self.on_position(data),
                    t if t == atoms.xdnd_leave => self.on_leave(),
                    t if t == atoms.xdnd_drop => self.on_drop(data),
                    t if t == atoms.xdnd_status => self.on_status(data),
                    t if t == atoms.xdnd_finished => self.on_finished(data),
                    _ => {}
                }
            }
            Event::SelectionNotify(notify) => self.on_selection_notify(notify.property),
            Event::SelectionRequest(request) => self.on_selection_request(request),
            Event::SelectionClear(_) => {
                // Someone else took `XdndSelection`. Our drag is over whether
                // we like it or not; report it so the source widget's
                // `on_drag_ended` still fires exactly once.
                if self.outbound.is_some() {
                    self.finish_outbound(DropOutcome::Cancelled);
                }
            }
            Event::PropertyNotify(notify) => {
                if notify.window == self.proxy
                    && notify.atom == atoms.teksilo_transfer
                    && notify.state == Property::NEW_VALUE
                {
                    self.on_incr_chunk();
                } else if notify.state == Property::DELETE {
                    // A requestor consumed an INCR chunk: send the next one.
                    self.on_incr_property_deleted(notify.window, notify.atom);
                }
            }
            _ => {}
        }
    }

    // ---------- inbound (we are the target) ----------

    fn on_enter(&mut self, data: [u32; 5]) {
        let Some(enter) = xdnd::decode_enter(data) else {
            // Version below 3 — no live toolkit emits these, and the
            // pre-v3 layout differs enough that guessing would be wrong.
            return;
        };
        let mut types = enter.types;
        if enter.more_types {
            // Only the first three types travel inline; the rest live in a
            // property on the source window. Missing them would silently drop
            // support for anything a rich source offers beyond its top three.
            types = self
                .conn
                .get_property_full(
                    enter.source,
                    self.conn.atoms().xdnd_type_list,
                    AtomEnum::ATOM.into(),
                )
                .ok()
                .flatten()
                .map(|value| value.as_u32s())
                .unwrap_or(types);
        }
        let chosen = xdnd::choose_type(&types, &self.conn.atoms().preferred_targets());
        self.inbound = Some(Inbound {
            source: enter.source,
            version: enter.version,
            types,
            chosen,
            position: Point::new(0.0, 0.0),
            entered: false,
            incr: None,
            dropping: false,
        });
    }

    fn on_position(&mut self, data: [u32; 5]) {
        let position = xdnd::decode_position(data);
        let Some(point) = self.to_window_logical(position.root_x, position.root_y) else {
            return;
        };

        // Take what the borrow needs, then release it: naming the formats
        // requires `&self` for atom lookups, which cannot overlap the mutable
        // borrow of `self.inbound`.
        let (accept, first, types) = {
            let Some(inbound) = self.inbound.as_mut() else {
                return;
            };
            inbound.position = point;
            let first = !inbound.entered;
            inbound.entered = true;
            let types = first.then(|| inbound.types.clone());
            (inbound.chosen.is_some(), first, types)
        };
        let formats = types.map(|types| self.type_names(&types));

        // Answer immediately: XDND requires a status per position, and we
        // cannot round-trip to the UI thread synchronously to ask the widget
        // under the cursor. So this advertises *format* compatibility; the
        // widget tree still decides whether it accepts, and a rejected drop
        // simply produces no drop handler call.
        let action = self.conn.atoms().xdnd_action_copy;
        self.send_to_source(
            self.conn.atoms().xdnd_status,
            xdnd::encode_status(self.toplevel, accept, action),
        );

        if first {
            // Bytes are not fetched during hover — a speculative
            // `ConvertSelection` per motion would be wasteful and some sources
            // refuse it. `formats` is exactly what `ExternalDropData` carries
            // for this case, and what `DropZone` validates on.
            self.post(ExternalDragEvent::Entered {
                data: ExternalDropData {
                    formats: formats.unwrap_or_default(),
                    ..Default::default()
                },
                position: point,
            });
        } else {
            self.post(ExternalDragEvent::Moved { position: point });
        }
    }

    fn on_leave(&mut self) {
        if self.inbound.take().is_some() {
            self.post(ExternalDragEvent::Left);
        }
    }

    fn on_drop(&mut self, data: [u32; 5]) {
        let drop = xdnd::decode_drop(data);
        let Some(inbound) = self.inbound.as_mut() else {
            return;
        };
        let Some(chosen) = inbound.chosen else {
            // Nothing we can read: finish the handshake honestly and end the
            // session, rather than leaving the source waiting.
            let (version, source) = (inbound.version, inbound.source);
            self.finish_inbound(source, version, false);
            self.inbound = None;
            self.post(ExternalDragEvent::Left);
            return;
        };
        inbound.dropping = true;
        let source = inbound.source;

        // The timestamp must be the one from `XdndDrop`, never `CurrentTime`:
        // it is how the source tells this request from a stale one belonging
        // to a drag the user already abandoned.
        let atoms = self.conn.atoms();
        let requested = self
            .conn
            .conn()
            .convert_selection(
                self.proxy,
                atoms.xdnd_selection,
                chosen,
                atoms.teksilo_transfer,
                drop.time,
            )
            .map(|cookie| cookie.check().is_ok())
            .unwrap_or(false);
        if !requested {
            let (version, source) = self
                .inbound
                .as_ref()
                .map_or((xdnd::XDND_VERSION, x11rb::NONE), |i| (i.version, i.source));
            self.finish_inbound(source, version, false);
            self.inbound = None;
            self.post(ExternalDragEvent::Left);
            return;
        }
        let _ = self.conn.flush();
        let _ = source;
    }

    fn on_selection_notify(&mut self, property: Atom) {
        // Only meaningful for a drop we asked to read.
        if !self.inbound.as_ref().is_some_and(|i| i.dropping) {
            return;
        }
        if property == x11rb::NONE {
            // The source declined to convert. End the session without
            // fabricating an empty drop.
            let (version, source) = self
                .inbound
                .as_ref()
                .map_or((xdnd::XDND_VERSION, x11rb::NONE), |i| (i.version, i.source));
            self.finish_inbound(source, version, false);
            self.inbound = None;
            self.post(ExternalDragEvent::Left);
            return;
        }

        let value = self
            .conn
            .get_property_and_delete(self.proxy, self.conn.atoms().teksilo_transfer)
            .ok()
            .flatten();
        let Some(value) = value else {
            self.complete_drop(Vec::new());
            return;
        };

        if value.type_ == self.conn.atoms().incr {
            // Large payload: the property holds only a size hint and the real
            // bytes arrive as a series of property writes, each acknowledged
            // by our deleting it (which `get_property_and_delete` above just
            // did, releasing the first chunk).
            let expected = value.as_u32().unwrap_or(0) as usize;
            if let Some(inbound) = self.inbound.as_mut() {
                inbound.incr = Some(xdnd::IncrAssembler::new(expected));
            }
            let _ = self.conn.flush();
            return;
        }

        self.complete_drop(value.bytes);
    }

    fn on_incr_chunk(&mut self) {
        if self.inbound.as_ref().is_none_or(|i| i.incr.is_none()) {
            return;
        }
        let Some(value) = self
            .conn
            .get_property_and_delete(self.proxy, self.conn.atoms().teksilo_transfer)
            .ok()
            .flatten()
        else {
            return;
        };
        let _ = self.conn.flush();

        let complete = self
            .inbound
            .as_mut()
            .and_then(|i| i.incr.as_mut())
            .map(|incr| incr.push(&value.bytes))
            .unwrap_or(false);

        if complete {
            let bytes = self
                .inbound
                .as_mut()
                .and_then(|i| i.incr.take())
                .map(xdnd::IncrAssembler::finish)
                .unwrap_or_default();
            self.complete_drop(bytes);
        }
    }

    /// Turn transferred bytes into an [`ExternalDropData`], post the drop, and
    /// close the XDND handshake.
    fn complete_drop(&mut self, bytes: Vec<u8>) {
        let Some(inbound) = self.inbound.take() else {
            return;
        };
        let chosen = inbound.chosen.unwrap_or(x11rb::NONE);
        let atoms = self.conn.atoms();

        let text = String::from_utf8_lossy(&bytes).into_owned();
        let mut data = if chosen == atoms.text_uri_list {
            // Shared with every other backend, so a file dropped on X11 lands
            // with exactly the same path as on Wayland or macOS.
            ExternalDropData::from_uri_list(&text)
        } else {
            ExternalDropData {
                text: Some(text.clone()),
                ..Default::default()
            }
        };
        if let Some(name) = self.atom_name(chosen) {
            data.mime.insert(name, bytes);
        }
        data.formats = self.type_names(&inbound.types);

        let accepted = !data.files.is_empty() || data.text.is_some() || !data.uris.is_empty();
        self.post(ExternalDragEvent::Dropped {
            data,
            position: inbound.position,
        });
        self.finish_inbound(inbound.source, inbound.version, accepted);
    }

    /// Send `XdndFinished` to `source`, closing the transaction so it can
    /// release the selection.
    ///
    /// The source window is passed in rather than read back from
    /// `self.inbound`: the drop path takes the session *before* finishing (it
    /// needs to own the position and types to build the payload), so reading it
    /// here would find `None` and silently skip the handshake — leaving the
    /// source holding `XdndSelection` and waiting for a confirmation that never
    /// arrives.
    fn finish_inbound(&mut self, source: Window, version: u32, accepted: bool) {
        if source == x11rb::NONE {
            return;
        }
        let action = self.conn.atoms().xdnd_action_copy;
        let data = xdnd::encode_finished(self.toplevel, version, accepted, action);
        let atom = self.conn.atoms().xdnd_finished;
        // Addressed to the source (see `send_to_source`); our own window is in
        // `data[0]`, which `encode_finished` already placed there.
        let _ = self
            .conn
            .send_client_message(source, source, atom, data, EventMask::NO_EVENT);
        let _ = self.conn.flush();
    }

    /// Send a target→source message for the current inbound session.
    ///
    /// The `window` field names the **source** — the recipient — not us. Our
    /// own window travels in `data[0]`. winit's `send_status` and Paul Sheer's
    /// reference `xdnd.c` both do this, and sources that route replies by
    /// `xclient.window` (GTK matches it against its drag context) discard a
    /// message addressed any other way.
    fn send_to_source(&self, type_: Atom, data: [u32; 5]) {
        let Some(source) = self.inbound.as_ref().map(|i| i.source) else {
            return;
        };
        let _ = self
            .conn
            .send_client_message(source, source, type_, data, EventMask::NO_EVENT);
        let _ = self.conn.flush();
    }

    // ---------- outbound (we are the source) ----------

    fn begin_outbound(&mut self, data: OutboundDragData) {
        match self.try_begin_outbound(data) {
            Ok(()) => {}
            Err(_) => {
                // Report the failure rather than leaving the source widget's
                // `on_drag_ended` unfired and its payload stashed forever.
                self.outbound = None;
                self.post(ExternalDragEvent::DragEnded {
                    outcome: DropOutcome::Cancelled,
                });
            }
        }
    }

    fn try_begin_outbound(&mut self, data: OutboundDragData) -> Result<(), X11Error> {
        let mimes = outbound_mimes(&data);
        let mut types = Vec::with_capacity(mimes.len());
        for mime in mimes {
            let atom = match self.conn.atoms().atom_for_mime(&mime) {
                Some(atom) => atom,
                None => {
                    self.conn
                        .conn()
                        .intern_atom(false, mime.as_bytes())?
                        .reply()?
                        .atom
                }
            };
            types.push((mime, atom));
        }
        if types.is_empty() {
            return Err(X11Error::Timeout(
                "an outbound payload with at least one MIME type",
            ));
        }

        // Selection ownership must be stamped with a real server time so a
        // target can reject a request from an abandoned drag.
        let time = self.conn.fetch_timestamp(self.proxy)?;
        let atoms = self.conn.atoms().clone();
        self.conn
            .conn()
            .set_selection_owner(self.proxy, atoms.xdnd_selection, time)?
            .check()?;
        if self
            .conn
            .conn()
            .get_selection_owner(atoms.xdnd_selection)?
            .reply()?
            .owner
            != self.proxy
        {
            return Err(X11Error::Timeout("XdndSelection ownership"));
        }

        // Sources with more than three types must publish the full list.
        let type_atoms: Vec<u32> = types.iter().map(|(_, atom)| *atom).collect();
        self.conn.set_property32(
            self.proxy,
            atoms.xdnd_type_list,
            AtomEnum::ATOM.into(),
            &type_atoms,
        )?;
        // Copy only, never Move: advertising Move would let the destination
        // physically relocate a dragged file. Same policy as every other
        // Teksilo backend.
        self.conn.set_property32(
            self.proxy,
            atoms.xdnd_action_list,
            AtomEnum::ATOM.into(),
            &[atoms.xdnd_action_copy],
        )?;
        self.conn.flush()?;

        self.outbound = Some(Outbound {
            data,
            types,
            time,
            target: None,
            send_to: None,
            version: xdnd::XDND_VERSION,
            accepted: false,
            dropped_at: None,
            resolved_for: None,
        });
        Ok(())
    }

    /// One tick of the outbound drag: sample the pointer, follow the target
    /// under it, and finish when the button comes up.
    fn pump_outbound(&mut self) {
        if self.outbound.is_none() {
            return;
        }
        // Past the drop, we are only waiting for `XdndFinished`.
        if let Some(started) = self.outbound.as_ref().and_then(|o| o.dropped_at) {
            if started.elapsed() > FINISH_TIMEOUT {
                // The target accepted the drop but never confirmed. The data
                // most likely arrived, so report the copy rather than claiming
                // a cancel that did not happen.
                self.finish_outbound(DropOutcome::OsCopy);
            }
            return;
        }

        let pointer = self
            .conn
            .conn()
            .query_pointer(self.conn.root())
            .ok()
            .and_then(|cookie| cookie.reply().ok());
        let Some(pointer) = pointer else {
            // The pointer is unreachable (server gone, or our connection is
            // dying): end the drag rather than spin.
            self.finish_outbound(DropOutcome::Cancelled);
            return;
        };

        // Button 1 still down ⇒ the drag continues. `QueryPointer` reports the
        // live button state regardless of who holds the grab, which is exactly
        // why this backend needs no grab of its own.
        let button_held = pointer.mask.contains(KeyButMask::BUTTON1);
        let (root_x, root_y) = (pointer.root_x, pointer.root_y);

        if !button_held {
            self.release_outbound();
            return;
        }

        // Resolving a target costs a tree descent plus a property read per
        // ancestor — dozens of blocking round trips. `QueryPointer` has already
        // told us which child of the root the pointer is over, and the server
        // recomputes that per call (so stacking changes are picked up), which
        // makes it a sound cache key: while it is unchanged the resolution
        // cannot have changed either. Staying over one window therefore costs
        // exactly the one QueryPointer above.
        let root_child = pointer.child;
        let cached = self
            .outbound
            .as_ref()
            .is_some_and(|o| o.resolved_for == Some(root_child));

        if !cached {
            let found = self.find_xdnd_target(root_x, root_y);
            let previous = self
                .outbound
                .as_ref()
                .and_then(|o| Some((o.send_to?, o.target?)));

            // Leave goes to the *previous* proxy, not the previous target: a
            // proxied window is not listening on its own id, so addressing it
            // there would strand it showing drop feedback for the rest of the
            // drag.
            if previous.map(|(_, target)| target) != found.map(|(target, ..)| target)
                && let Some((send_to, target)) = previous
            {
                self.send_outbound(
                    send_to,
                    target,
                    self.conn.atoms().xdnd_leave,
                    xdnd::encode_leave(self.proxy),
                );
            }

            let entering = previous.map(|(_, target)| target) != found.map(|(target, ..)| target);
            if let Some(outbound) = self.outbound.as_mut() {
                outbound.resolved_for = Some(root_child);
                match found {
                    Some((target, send_to, version)) => {
                        outbound.target = Some(target);
                        outbound.send_to = Some(send_to);
                        outbound.version = version;
                    }
                    None => {
                        outbound.target = None;
                        outbound.send_to = None;
                    }
                }
                if entering {
                    outbound.accepted = false;
                }
            }

            if entering && let Some((target, send_to, version)) = found {
                let type_atoms: Vec<Atom> = self
                    .outbound
                    .as_ref()
                    .map(|o| o.types.iter().map(|(_, atom)| *atom).collect())
                    .unwrap_or_default();
                let data = xdnd::encode_enter(self.proxy, version, &type_atoms);
                self.send_outbound(send_to, target, self.conn.atoms().xdnd_enter, data);
            }
        }

        if let Some((send_to, target)) = self.outbound_address() {
            let time = self.outbound.as_ref().map_or(0, |o| o.time);
            let action = self.conn.atoms().xdnd_action_copy;
            let data = xdnd::encode_position(self.proxy, root_x, root_y, time, action);
            self.send_outbound(send_to, target, self.conn.atoms().xdnd_position, data);
        }
    }

    /// `(where to send, which window to name)` for the current outbound
    /// target, or `None` when the pointer is over nothing XDND-aware.
    fn outbound_address(&self) -> Option<(Window, Window)> {
        let outbound = self.outbound.as_ref()?;
        Some((outbound.send_to?, outbound.target?))
    }

    /// The pointer came up: drop on the current target, or cancel.
    fn release_outbound(&mut self) {
        let (accepted, time) = match self.outbound.as_ref() {
            Some(o) => (o.accepted, o.time),
            None => return,
        };
        match (self.outbound_address(), accepted) {
            (Some((send_to, target)), true) => {
                let data = xdnd::encode_drop(self.proxy, time);
                self.send_outbound(send_to, target, self.conn.atoms().xdnd_drop, data);
                if let Some(outbound) = self.outbound.as_mut() {
                    outbound.dropped_at = Some(Instant::now());
                }
            }
            (Some((send_to, target)), false) => {
                self.send_outbound(
                    send_to,
                    target,
                    self.conn.atoms().xdnd_leave,
                    xdnd::encode_leave(self.proxy),
                );
                self.finish_outbound(DropOutcome::Cancelled);
            }
            (None, _) => self.finish_outbound(DropOutcome::Cancelled),
        }
    }

    fn cancel_outbound(&mut self) {
        if let Some((send_to, target)) = self.outbound_address() {
            self.send_outbound(
                send_to,
                target,
                self.conn.atoms().xdnd_leave,
                xdnd::encode_leave(self.proxy),
            );
        }
        if self.outbound.is_some() {
            self.finish_outbound(DropOutcome::Cancelled);
        }
    }

    fn on_status(&mut self, data: [u32; 5]) {
        let status = xdnd::decode_status(data);
        // `data[0]` names the target precisely so a status queued by a window
        // we have already left can be discarded. Without this, dragging quickly
        // from a rejecting window into an accepting one can apply the old
        // rejection to the new target, turning the release into an XdndLeave
        // and silently losing the drop.
        if self
            .outbound
            .as_ref()
            .is_some_and(|o| o.target.is_some_and(|t| t != status.target))
        {
            return;
        }
        if let Some(outbound) = self.outbound.as_mut() {
            outbound.accepted = status.accepted;
        }
    }

    fn on_finished(&mut self, data: [u32; 5]) {
        let version = self
            .outbound
            .as_ref()
            .map_or(xdnd::XDND_VERSION, |o| o.version);
        let finished = xdnd::decode_finished(data, version);
        if self.outbound.is_some() {
            let outcome = if finished.accepted {
                DropOutcome::OsCopy
            } else {
                DropOutcome::Cancelled
            };
            self.finish_outbound(outcome);
        }
    }

    /// End the outbound session and report the outcome exactly once.
    fn finish_outbound(&mut self, outcome: DropOutcome) {
        if self.outbound.take().is_none() {
            return;
        }
        // Release the selection so a later drag (ours or anyone's) starts clean.
        let selection = self.conn.atoms().xdnd_selection;
        ignore_errors(self.conn.conn().set_selection_owner(
            x11rb::NONE,
            selection,
            x11rb::CURRENT_TIME,
        ));
        let _ = self.conn.flush();
        self.post(ExternalDragEvent::DragEnded { outcome });
    }

    /// Send a source→target message.
    ///
    /// `send_to` and `target` differ whenever the target names an `XdndProxy`,
    /// and the spec is explicit that only the *address* changes: "the
    /// appropriate field of the client messages, `window` or `data.l[0]`, must
    /// contain the ID of the window in which the mouse is located, not the
    /// proxy window that is receiving the messages." Naming the proxy instead
    /// is what makes a proxy fronting several windows unable to route the drop
    /// (the bug Chromium tracks as crbug.com/41278320).
    fn send_outbound(&self, send_to: Window, target: Window, type_: Atom, data: [u32; 5]) {
        let _ = self
            .conn
            .send_client_message(send_to, target, type_, data, EventMask::NO_EVENT);
        let _ = self.conn.flush();
    }

    /// Find the XDND-aware window under a root-relative point.
    ///
    /// Descends the window tree to the deepest window containing the point,
    /// then walks back up looking for `XdndAware` — the algorithm GTK and Qt
    /// both use, because the toplevel that carries the property is usually an
    /// ancestor of whatever subwindow is actually under the cursor.
    ///
    /// Returns `(semantic target, where to send, negotiated version)`. The two
    /// windows differ when the target names an `XdndProxy`: messages go to the
    /// proxy, but the payload still names the real window — which is exactly
    /// what makes our own inbound path work.
    fn find_xdnd_target(&self, root_x: i16, root_y: i16) -> Option<(Window, Window, u32)> {
        let root = self.conn.root();
        let mut current = root;
        // Bounded: a pathological or cyclic tree must not spin forever.
        for _ in 0..32 {
            let reply = self
                .conn
                .conn()
                .translate_coordinates(root, current, root_x, root_y)
                .ok()?
                .reply()
                .ok()?;
            if reply.child == x11rb::NONE {
                break;
            }
            current = reply.child;
        }

        let atoms = self.conn.atoms();
        let mut candidate = current;
        for _ in 0..32 {
            if candidate == x11rb::NONE || candidate == root {
                return None;
            }
            if let Some(version) = self
                .conn
                .get_property_full(candidate, atoms.xdnd_aware, AtomEnum::ATOM.into())
                .ok()
                .flatten()
                .and_then(|value| value.as_u32())
                .and_then(xdnd::negotiate_version)
            {
                let proxy = self
                    .conn
                    .get_property_full(candidate, atoms.xdnd_proxy, AtomEnum::WINDOW.into())
                    .ok()
                    .flatten()
                    .and_then(|value| value.as_u32());
                let proxy_self = proxy.and_then(|p| {
                    self.conn
                        .get_property_full(p, atoms.xdnd_proxy, AtomEnum::WINDOW.into())
                        .ok()
                        .flatten()
                        .and_then(|value| value.as_u32())
                });
                let send_to = xdnd::resolve_proxy(candidate, proxy, proxy_self);
                return Some((candidate, send_to, version));
            }
            candidate = self
                .conn
                .conn()
                .query_tree(candidate)
                .ok()?
                .reply()
                .ok()
                .map(|reply| reply.parent)?;
        }
        None
    }

    // ---------- serving the selection ----------

    fn on_selection_request(&mut self, request: x11rb::protocol::xproto::SelectionRequestEvent) {
        let atoms = self.conn.atoms().clone();
        if request.selection != atoms.xdnd_selection {
            return;
        }
        // ICCCM: a requestor that sends no property is using the obsolete
        // pre-ICCCM convention where the target atom doubles as the property.
        let property = if request.property == x11rb::NONE {
            request.target
        } else {
            request.property
        };

        let served = self.serve_selection(&request, property, &atoms);
        // Answer either way: silence would leave the requestor blocked until
        // its own timeout, stalling the drop for seconds.
        let notify = SelectionNotifyEvent {
            response_type: x11rb::protocol::xproto::SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: request.time,
            requestor: request.requestor,
            selection: request.selection,
            target: request.target,
            property: if served { property } else { x11rb::NONE },
        };
        ignore_errors(self.conn.conn().send_event(
            false,
            request.requestor,
            EventMask::NO_EVENT,
            notify,
        ));
        let _ = self.conn.flush();
    }

    fn serve_selection(
        &mut self,
        request: &x11rb::protocol::xproto::SelectionRequestEvent,
        property: Atom,
        atoms: &crate::x11::Atoms,
    ) -> bool {
        let Some(outbound) = self.outbound.as_ref() else {
            return false;
        };

        if request.target == atoms.targets {
            let mut list: Vec<u32> = outbound.types.iter().map(|(_, atom)| *atom).collect();
            list.push(atoms.targets);
            return self
                .conn
                .set_property32(request.requestor, property, AtomEnum::ATOM.into(), &list)
                .is_ok();
        }
        if request.target == atoms.timestamp {
            return self
                .conn
                .set_property32(
                    request.requestor,
                    property,
                    AtomEnum::INTEGER.into(),
                    &[outbound.time],
                )
                .is_ok();
        }

        let Some((mime, _)) = outbound
            .types
            .iter()
            .find(|(_, atom)| *atom == request.target)
        else {
            return false;
        };
        let bytes = outbound_bytes(&outbound.data, mime);

        if bytes.len() <= INCR_THRESHOLD {
            return self
                .conn
                .set_property8(request.requestor, property, request.target, &bytes)
                .is_ok();
        }

        // Too large for one property write: announce INCR, then feed chunks as
        // the requestor deletes the property. This needs PropertyNotify from a
        // window we do not own, so select for it first.
        let selected = self
            .conn
            .conn()
            .change_window_attributes(
                request.requestor,
                &x11rb::protocol::xproto::ChangeWindowAttributesAux::new()
                    .event_mask(EventMask::PROPERTY_CHANGE),
            )
            .map(|cookie| cookie.check().is_ok())
            .unwrap_or(false);
        if !selected {
            return false;
        }
        let total = [bytes.len() as u32];
        if self
            .conn
            .set_property32(request.requestor, property, atoms.incr, &total)
            .is_err()
        {
            return false;
        }
        let chunk = self.max_chunk_bytes();
        self.incr_sends.push(IncrSend {
            requestor: request.requestor,
            property,
            type_: request.target,
            remaining: bytes,
            chunk,
        });
        true
    }

    /// Feed the next `INCR` chunk after the requestor deleted the property.
    fn on_incr_property_deleted(&mut self, window: Window, property: Atom) {
        let Some(index) = self
            .incr_sends
            .iter()
            .position(|send| send.requestor == window && send.property == property)
        else {
            return;
        };

        let (requestor, prop, type_, chunk, done) = {
            let send = &mut self.incr_sends[index];
            let take = send.chunk.min(send.remaining.len());
            let chunk: Vec<u8> = send.remaining.drain(..take).collect();
            // A zero-length write terminates the transfer, so the empty final
            // chunk is not a no-op — it is the end marker.
            let done = chunk.is_empty();
            (send.requestor, send.property, send.type_, chunk, done)
        };
        let _ = self.conn.set_property8(requestor, prop, type_, &chunk);
        let _ = self.conn.flush();
        if done {
            self.incr_sends.remove(index);
        }
    }

    /// Largest payload that fits in one property write on this connection.
    fn max_chunk_bytes(&self) -> usize {
        // `maximum_request_length` counts 4-byte units and must also hold the
        // request header; leave generous headroom rather than computing the
        // exact header size, since the only cost is one extra round trip.
        let units = self.conn.conn().setup().maximum_request_length as usize;
        (units.saturating_mul(4) / 2).clamp(4096, INCR_THRESHOLD)
    }

    // ---------- helpers ----------

    fn atom_name(&self, atom: Atom) -> Option<String> {
        if atom == x11rb::NONE {
            return None;
        }
        let reply = self.conn.conn().get_atom_name(atom).ok()?.reply().ok()?;
        Some(String::from_utf8_lossy(&reply.name).into_owned())
    }

    /// MIME-type names for a set of atoms, for `ExternalDropData::formats`.
    fn type_names(&self, types: &[Atom]) -> Vec<String> {
        types
            .iter()
            .filter_map(|&atom| self.atom_name(atom))
            .collect()
    }

    /// Remove everything we published, so the toplevel does not outlive its
    /// proxy pointing at a destroyed window.
    fn teardown(&mut self) {
        if self.outbound.is_some() {
            self.finish_outbound(DropOutcome::Cancelled);
        }
        let atoms = self.conn.atoms().clone();
        let _ = self
            .conn
            .conn()
            .delete_property(self.toplevel, atoms.xdnd_proxy);
        let _ = self.conn.conn().destroy_window(self.proxy);
        let _ = self.conn.flush();
    }
}

#[cfg(test)]
mod tests;
