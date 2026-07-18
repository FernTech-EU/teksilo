// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! End-to-end XDND tests against a **real** X server.
//!
//! The wire protocol cannot be exercised by the pure unit tests in
//! [`crate::x11::xdnd`] — those cover encoding, version negotiation and
//! `text/uri-list` handling, but not whether a live source can actually find
//! our proxy window, complete the handshake, and have the bytes arrive. There
//! is no in-process X server or protocol double for Rust, so these tests drive
//! an actual server and are `#[ignore]`d by default:
//!
//! ```text
//! WAYLAND_DISPLAY= cargo test -p bastyde-platform -- --ignored x11_dnd
//! ```
//!
//! CI runs them under `xvfb-run` (see `.github/workflows/ci.yml`). No window
//! manager is required — XDND is pure client-to-client messaging and works with
//! no WM running at all, unlike the custom title bar.
//!
//! The harness plays the part GTK or Qt would: it resolves `XdndProxy` exactly
//! as `xdnd_check_dest` does, sends the real `ClientMessage`s, and serves the
//! selection. What it asserts is the [`ExternalDragEvent`]s the backend posts,
//! which is precisely the contract the widget tree consumes.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bastyde_core::window::BastydeWindowId;
use bastyde_core::{AppEventPoster, SubscriptionId};
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WindowHandle, XlibDisplayHandle, XlibWindowHandle,
};
use x11rb::protocol::xproto::{
    ConnectionExt as _, CreateWindowAux, EventMask, SelectionNotifyEvent, WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;

use super::*;

/// How long a test waits for an expected event before failing. Generous: CI
/// under Xvfb is slower than a desktop, and a flaky timeout would be worse
/// than a slow test.
const DEADLINE: Duration = Duration::from_secs(5);

/// Serialises the tests in this module.
///
/// Not a nicety: `XdndSelection` is a **single, display-global** selection, so
/// only one XDND drag can be in flight at a time — that is the protocol, not a
/// limitation of this harness. Two tests owning it concurrently steal it from
/// each other and the loser's `ConvertSelection` is answered by the wrong
/// source, which surfaces as a drop that silently turns into a cancel.
fn x_server_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ============================================================
// Harness
// ============================================================

/// Records every payload the backend posts, so a test can assert on the
/// `ExternalDragEvent` sequence the widget tree would have seen.
#[derive(Default)]
struct CapturingPoster {
    events: Mutex<Vec<ExternalDragEvent>>,
}

impl AppEventPoster for CapturingPoster {
    fn post_subscription_event(&self, _sub: SubscriptionId, _event: Box<dyn std::any::Any + Send>) {
    }

    fn post_external(&self, payload: Box<dyn std::any::Any + Send>) {
        if let Ok(payload) = payload.downcast::<ExternalDndEventPayload>() {
            self.events.lock().unwrap().push(payload.event);
        }
    }
}

impl CapturingPoster {
    fn events(&self) -> Vec<ExternalDragEvent> {
        self.events.lock().unwrap().clone()
    }

    /// Block until `predicate` matches one of the recorded events.
    fn wait_for(
        &self,
        what: &str,
        predicate: impl Fn(&ExternalDragEvent) -> bool,
    ) -> ExternalDragEvent {
        let start = Instant::now();
        while start.elapsed() < DEADLINE {
            if let Some(found) = self.events().into_iter().find(&predicate) {
                return found;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("timed out waiting for {what}; saw: {:?}", self.events());
    }
}

/// Stands in for winit's window: carries the X11 handles for a window the test
/// created itself, so the backend can be exercised with no GUI stack at all.
struct FakeWindow {
    window: u32,
    screen: i32,
}

impl HasWindowHandle for FakeWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let handle = XlibWindowHandle::new(self.window as std::ffi::c_ulong);
        // SAFETY: the handle describes a window this test created on a live
        // connection and keeps alive for the whole test.
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Xlib(handle)) })
    }
}

impl HasDisplayHandle for FakeWindow {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let handle = XlibDisplayHandle::new(None, self.screen);
        // SAFETY: a null display pointer is explicitly allowed by
        // `XlibDisplayHandle::new`; the X11 backend only ever reads the
        // *variant* from the display handle (to confirm this is X11) and opens
        // its own connection, never dereferencing this pointer.
        Ok(unsafe { DisplayHandle::borrow_raw(RawDisplayHandle::Xlib(handle)) })
    }
}

/// A test-owned target window plus the backend attached to it.
struct Target {
    conn: x11rb::rust_connection::RustConnection,
    window: u32,
    root: u32,
    screen: i32,
    poster: Arc<CapturingPoster>,
    _guard: Box<dyn ExternalDndGuard>,
}

impl Target {
    /// Create a mapped window and attach the X11 DnD backend to it, waiting
    /// until the backend has published its `XdndProxy`.
    fn new() -> Self {
        let (conn, screen_num) = x11rb::connect(None).expect("connect to $DISPLAY");
        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;
        let window = conn.generate_id().unwrap();
        conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            window,
            root,
            0,
            0,
            400,
            300,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new()
                .background_pixel(screen.white_pixel)
                // Override-redirect keeps a window manager from reparenting or
                // repositioning the window, so the coordinate assertions below
                // are about our code, not about the WM's placement policy.
                .override_redirect(1)
                .event_mask(EventMask::PROPERTY_CHANGE),
        )
        .unwrap()
        .check()
        .unwrap();
        conn.map_window(window).unwrap().check().unwrap();
        conn.flush().unwrap();

        let poster = Arc::new(CapturingPoster::default());
        let fake = FakeWindow {
            window,
            screen: screen_num as i32,
        };
        let parent = bastyde_core::raw_handle::ParentHandle::from_window(&fake).unwrap();
        let guard = X11ExternalDndBackend::new().attach(
            parent,
            BastydeWindowId::new(1),
            poster.clone() as Arc<dyn AppEventPoster>,
        );

        let target = Self {
            conn,
            window,
            root,
            screen: screen_num as i32,
            poster,
            _guard: guard,
        };
        target.await_proxy();
        target
    }

    fn atom(&self, name: &[u8]) -> u32 {
        self.conn
            .intern_atom(false, name)
            .unwrap()
            .reply()
            .unwrap()
            .atom
    }

    fn read_window_property(&self, window: u32, property: u32) -> Option<u32> {
        let reply = self
            .conn
            .get_property(
                false,
                window,
                property,
                x11rb::protocol::xproto::AtomEnum::ANY,
                0,
                1,
            )
            .ok()?
            .reply()
            .ok()?;
        reply.value32()?.next()
    }

    /// Spin until the backend thread has installed `XdndProxy` on our window.
    fn await_proxy(&self) -> u32 {
        let proxy_atom = self.atom(b"XdndProxy");
        let start = Instant::now();
        while start.elapsed() < DEADLINE {
            if let Some(proxy) = self.read_window_property(self.window, proxy_atom) {
                return proxy;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("backend never published XdndProxy on the target window");
    }

    /// Where our window's origin sits in root coordinates, so a test can aim a
    /// drop at a known point *inside* it.
    fn origin(&self) -> (i16, i16) {
        let reply = self
            .conn
            .translate_coordinates(self.window, self.root, 0, 0)
            .unwrap()
            .reply()
            .unwrap();
        (reply.dst_x, reply.dst_y)
    }
}

/// Plays an XDND **source**, the way GTK or Qt would.
struct Source {
    conn: x11rb::rust_connection::RustConnection,
    window: u32,
    payload: Vec<u8>,
    uri_list: u32,
}

impl Source {
    fn new(screen_num: usize, payload: &str) -> Self {
        let (conn, _) = x11rb::connect(None).expect("connect to $DISPLAY");
        let screen = &conn.setup().roots[screen_num];
        let window = conn.generate_id().unwrap();
        conn.create_window(
            0,
            window,
            screen.root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &CreateWindowAux::new()
                .override_redirect(1)
                .event_mask(EventMask::PROPERTY_CHANGE),
        )
        .unwrap()
        .check()
        .unwrap();
        let uri_list = conn
            .intern_atom(false, b"text/uri-list")
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        let selection = conn
            .intern_atom(false, b"XdndSelection")
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        conn.set_selection_owner(window, selection, x11rb::CURRENT_TIME)
            .unwrap()
            .check()
            .unwrap();
        conn.flush().unwrap();
        Self {
            conn,
            window,
            payload: payload.as_bytes().to_vec(),
            uri_list,
        }
    }

    fn atom(&self, name: &[u8]) -> u32 {
        self.conn
            .intern_atom(false, name)
            .unwrap()
            .reply()
            .unwrap()
            .atom
    }

    /// Resolve a target's `XdndProxy` the way GTK's `xdnd_check_dest` does,
    /// including the spec's self-pointing validation.
    fn resolve_proxy(&self, target: u32) -> u32 {
        let proxy_atom = self.atom(b"XdndProxy");
        let read = |window: u32| -> Option<u32> {
            self.conn
                .get_property(
                    false,
                    window,
                    proxy_atom,
                    x11rb::protocol::xproto::AtomEnum::ANY,
                    0,
                    1,
                )
                .ok()?
                .reply()
                .ok()?
                .value32()?
                .next()
        };
        let proxy = read(target);
        let proxy_self = proxy.and_then(read);
        crate::x11::xdnd::resolve_proxy(target, proxy, proxy_self)
    }

    fn send(&self, to: u32, window_field: u32, type_: u32, data: [u32; 5]) {
        let event = x11rb::protocol::xproto::ClientMessageEvent::new(32, window_field, type_, data);
        self.conn
            .send_event(false, to, EventMask::NO_EVENT, event)
            .unwrap()
            .check()
            .unwrap();
        self.conn.flush().unwrap();
    }

    /// Serve one `SelectionRequest` and answer `XdndFinished`, returning the
    /// finish message's `window` field once it arrives.
    ///
    /// The `window` field is returned rather than ignored because the protocol
    /// routes target→source replies by it, and matching on message type alone
    /// is precisely what let a misaddressed reply pass unnoticed.
    fn pump(&self, deadline: Duration) -> Option<u32> {
        let finished_atom = self.atom(b"XdndFinished");
        let start = Instant::now();
        while start.elapsed() < deadline {
            match self.conn.poll_for_event() {
                Ok(Some(x11rb::protocol::Event::SelectionRequest(request))) => {
                    let property = if request.property == x11rb::NONE {
                        request.target
                    } else {
                        request.property
                    };
                    let ok = request.target == self.uri_list
                        && self
                            .conn
                            .change_property8(
                                x11rb::protocol::xproto::PropMode::REPLACE,
                                request.requestor,
                                property,
                                request.target,
                                &self.payload,
                            )
                            .map(|c| c.check().is_ok())
                            .unwrap_or(false);
                    let notify = SelectionNotifyEvent {
                        response_type: x11rb::protocol::xproto::SELECTION_NOTIFY_EVENT,
                        sequence: 0,
                        time: request.time,
                        requestor: request.requestor,
                        selection: request.selection,
                        target: request.target,
                        property: if ok { property } else { x11rb::NONE },
                    };
                    self.conn
                        .send_event(false, request.requestor, EventMask::NO_EVENT, notify)
                        .unwrap()
                        .check()
                        .unwrap();
                    self.conn.flush().unwrap();
                }
                Ok(Some(x11rb::protocol::Event::ClientMessage(msg)))
                    if msg.type_ == finished_atom =>
                {
                    return Some(msg.window);
                }
                Ok(Some(_)) => {}
                Ok(None) => std::thread::sleep(Duration::from_millis(2)),
                Err(_) => return None,
            }
        }
        None
    }

    /// Wait for an `XdndStatus`, returning its `window` field and accept bit.
    fn await_status(&self, deadline: Duration) -> Option<(u32, bool)> {
        let status_atom = self.atom(b"XdndStatus");
        let start = Instant::now();
        while start.elapsed() < deadline {
            match self.conn.poll_for_event() {
                Ok(Some(x11rb::protocol::Event::ClientMessage(msg)))
                    if msg.type_ == status_atom =>
                {
                    let data = msg.data.as_data32();
                    let decoded = crate::x11::xdnd::decode_status([
                        data[0], data[1], data[2], data[3], data[4],
                    ]);
                    return Some((msg.window, decoded.accepted));
                }
                Ok(Some(_)) => {}
                Ok(None) => std::thread::sleep(Duration::from_millis(2)),
                Err(_) => return None,
            }
        }
        None
    }
}

// ============================================================
// Tests
// ============================================================

/// The whole inbound path: a proxy-aware source finds our proxy, the handshake
/// completes, and the dropped file arrives at the right position with its path
/// decoded.
#[test]
#[ignore = "requires a live X server; run with --ignored"]
fn x11_dnd_inbound_file_drop() {
    let _x = x_server_lock();
    let target = Target::new();
    let source = Source::new(
        target.screen as usize,
        "file:///tmp/x11%20drop%20test.txt\r\n",
    );

    // The source must reach us *through* the proxy — that is the mechanism
    // under test. Talking to the toplevel directly would reach winit instead.
    let send_to = source.resolve_proxy(target.window);
    assert_ne!(
        send_to, target.window,
        "the source should have been redirected to the proxy"
    );

    let enter = source.atom(b"XdndEnter");
    let position = source.atom(b"XdndPosition");
    let drop = source.atom(b"XdndDrop");
    source.send(
        send_to,
        source.window,
        enter,
        crate::x11::xdnd::encode_enter(source.window, 5, &[source.uri_list]),
    );

    // Aim at (120, 80) inside the window, expressed in root coordinates the
    // way a real source would.
    let (ox, oy) = target.origin();
    source.send(
        send_to,
        source.window,
        position,
        crate::x11::xdnd::encode_position(
            source.window,
            ox + 120,
            oy + 80,
            x11rb::CURRENT_TIME,
            source.atom(b"XdndActionCopy"),
        ),
    );

    let entered = target.poster.wait_for("Entered", |e| {
        matches!(e, ExternalDragEvent::Entered { .. })
    });
    let ExternalDragEvent::Entered { data, position: at } = entered else {
        unreachable!()
    };
    assert_eq!(
        (at.x as i32, at.y as i32),
        (120, 80),
        "root coordinates must be reported relative to the window"
    );
    assert!(
        data.formats.iter().any(|f| f == "text/uri-list"),
        "the offered formats must be visible at hover, before any bytes move: {:?}",
        data.formats
    );

    source.send(
        send_to,
        source.window,
        drop,
        crate::x11::xdnd::encode_drop(source.window, x11rb::CURRENT_TIME),
    );

    // Serve the selection when the backend converts it, and wait for the
    // backend to close the handshake.
    let finished = source.pump(DEADLINE);

    let dropped = target.poster.wait_for("Dropped", |e| {
        matches!(e, ExternalDragEvent::Dropped { .. })
    });
    let ExternalDragEvent::Dropped { data, position: at } = dropped else {
        unreachable!()
    };
    assert_eq!(
        data.files,
        vec![std::path::PathBuf::from("/tmp/x11 drop test.txt")],
        "the percent-encoded path must be decoded"
    );
    assert_eq!((at.x as i32, at.y as i32), (120, 80));
    assert_eq!(
        finished,
        Some(source.window),
        "XdndFinished must be addressed to the source window: winit's send_status \
         and the reference xdnd.c both put the recipient there, and sources that \
         route replies by xclient.window discard anything else"
    );
}

/// A drag that leaves without dropping must end the session, or the widget
/// tree would be left showing hover feedback forever.
#[test]
#[ignore = "requires a live X server; run with --ignored"]
fn x11_dnd_inbound_leave_ends_the_session() {
    let _x = x_server_lock();
    let target = Target::new();
    let source = Source::new(target.screen as usize, "file:///tmp/a.txt\r\n");
    let send_to = source.resolve_proxy(target.window);

    let (ox, oy) = target.origin();
    source.send(
        send_to,
        source.window,
        source.atom(b"XdndEnter"),
        crate::x11::xdnd::encode_enter(source.window, 5, &[source.uri_list]),
    );
    source.send(
        send_to,
        source.window,
        source.atom(b"XdndPosition"),
        crate::x11::xdnd::encode_position(
            source.window,
            ox + 10,
            oy + 10,
            x11rb::CURRENT_TIME,
            source.atom(b"XdndActionCopy"),
        ),
    );
    target.poster.wait_for("Entered", |e| {
        matches!(e, ExternalDragEvent::Entered { .. })
    });

    source.send(
        send_to,
        source.window,
        source.atom(b"XdndLeave"),
        crate::x11::xdnd::encode_leave(source.window),
    );
    target
        .poster
        .wait_for("Left", |e| matches!(e, ExternalDragEvent::Left));
}

/// Motion after the first position must report `Moved`, not a second
/// `Entered` — a drop target tracks hover state off that distinction.
#[test]
#[ignore = "requires a live X server; run with --ignored"]
fn x11_dnd_inbound_reports_motion_after_entry() {
    let _x = x_server_lock();
    let target = Target::new();
    let source = Source::new(target.screen as usize, "file:///tmp/a.txt\r\n");
    let send_to = source.resolve_proxy(target.window);
    let (ox, oy) = target.origin();
    let copy = source.atom(b"XdndActionCopy");
    let position_atom = source.atom(b"XdndPosition");

    source.send(
        send_to,
        source.window,
        source.atom(b"XdndEnter"),
        crate::x11::xdnd::encode_enter(source.window, 5, &[source.uri_list]),
    );
    for (x, y) in [(10i16, 10i16), (40, 60)] {
        source.send(
            send_to,
            source.window,
            position_atom,
            crate::x11::xdnd::encode_position(
                source.window,
                ox + x,
                oy + y,
                x11rb::CURRENT_TIME,
                copy,
            ),
        );
    }

    let moved = target
        .poster
        .wait_for("Moved", |e| matches!(e, ExternalDragEvent::Moved { .. }));
    let ExternalDragEvent::Moved { position } = moved else {
        unreachable!()
    };
    assert_eq!((position.x as i32, position.y as i32), (40, 60));

    let entered_count = target
        .poster
        .events()
        .iter()
        .filter(|e| matches!(e, ExternalDragEvent::Entered { .. }))
        .count();
    assert_eq!(
        entered_count, 1,
        "entry must be reported exactly once per drag"
    );
}

/// The scale factor divides reported positions: X11 speaks physical pixels
/// only, and every `ExternalDragEvent` position is window-**logical**.
#[test]
#[ignore = "requires a live X server; run with --ignored"]
fn x11_dnd_positions_honour_the_scale_factor() {
    let _x = x_server_lock();
    let target = Target::new();
    target._guard.set_scale_factor(2.0);

    let source = Source::new(target.screen as usize, "file:///tmp/a.txt\r\n");
    let send_to = source.resolve_proxy(target.window);
    let (ox, oy) = target.origin();

    source.send(
        send_to,
        source.window,
        source.atom(b"XdndEnter"),
        crate::x11::xdnd::encode_enter(source.window, 5, &[source.uri_list]),
    );
    source.send(
        send_to,
        source.window,
        source.atom(b"XdndPosition"),
        crate::x11::xdnd::encode_position(
            source.window,
            ox + 200,
            oy + 100,
            x11rb::CURRENT_TIME,
            source.atom(b"XdndActionCopy"),
        ),
    );

    let entered = target.poster.wait_for("Entered", |e| {
        matches!(e, ExternalDragEvent::Entered { .. })
    });
    let ExternalDragEvent::Entered { position, .. } = entered else {
        unreachable!()
    };
    assert_eq!(
        (position.x as i32, position.y as i32),
        (100, 50),
        "200x100 physical at scale 2 is 100x50 logical"
    );
}

/// Dropping the guard must remove `XdndProxy` from the toplevel. A dangling
/// pointer to a destroyed proxy would make every future source fall back to
/// winit's files-only path — silently, and for the life of the window.
#[test]
#[ignore = "requires a live X server; run with --ignored"]
fn x11_dnd_detach_removes_the_proxy_property() {
    let _x = x_server_lock();
    let target = Target::new();
    let proxy_atom = target.atom(b"XdndProxy");
    assert!(
        target
            .read_window_property(target.window, proxy_atom)
            .is_some()
    );

    let Target {
        conn,
        window,
        _guard,
        ..
    } = target;
    drop(_guard);

    // `Drop` joins the backend thread, so the delete has been flushed — but it
    // was flushed from *its* connection and we read from ours, and X orders
    // requests only within a connection. Poll rather than race.
    let start = Instant::now();
    while start.elapsed() < DEADLINE {
        let reply = conn
            .get_property(
                false,
                window,
                proxy_atom,
                x11rb::protocol::xproto::AtomEnum::ANY,
                0,
                1,
            )
            .unwrap()
            .reply()
            .unwrap();
        if reply.value_len == 0 {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("XdndProxy must not outlive the backend that published it");
}

/// Target→source replies must name the **source** in the ClientMessage
/// `window` field, not our own window.
///
/// Sources route replies by `xclient.window` (GTK matches it against its drag
/// context), so a status addressed to ourselves is silently dropped and the
/// drag shows "rejected" feedback over a window that in fact accepts.
#[test]
#[ignore = "requires a live X server; run with --ignored"]
fn x11_dnd_status_is_addressed_to_the_source() {
    let _x = x_server_lock();
    let target = Target::new();
    let source = Source::new(target.screen as usize, "file:///tmp/a.txt\r\n");
    let send_to = source.resolve_proxy(target.window);
    let (ox, oy) = target.origin();

    source.send(
        send_to,
        source.window,
        source.atom(b"XdndEnter"),
        crate::x11::xdnd::encode_enter(source.window, 5, &[source.uri_list]),
    );
    source.send(
        send_to,
        source.window,
        source.atom(b"XdndPosition"),
        crate::x11::xdnd::encode_position(
            source.window,
            ox + 10,
            oy + 10,
            x11rb::CURRENT_TIME,
            source.atom(b"XdndActionCopy"),
        ),
    );

    let (window, accepted) = source.await_status(DEADLINE).expect("an XdndStatus reply");
    assert_eq!(
        window, source.window,
        "XdndStatus must be addressed to the source window, not the target's own"
    );
    assert!(accepted, "a text/uri-list offer is one we accept");
}
