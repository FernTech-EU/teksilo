// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The Wayland clipboard against a real compositor with no data-control
//! protocol: headless Mutter, the compositor of GNOME, with no X server.
//!
//! These tests are `#[ignore]`d: they need `mutter`, `dbus-daemon` and
//! `wl-clipboard` (`wl-copy`, `wl-paste`) installed, and a writable
//! `XDG_RUNTIME_DIR`. Run them with
//!
//! ```text
//! cargo test -p teksilo-platform --features clipboard --lib \
//!     clipboard::wayland::compositor_tests -- --ignored --test-threads=1
//! ```
//!
//! Each test starts a private `dbus-daemon` and a headless Mutter on a Wayland
//! socket of its own (`--no-x11`, and every child started without `DISPLAY`),
//! so it never touches the desktop it runs from. A headless Mutter's seat has
//! no keyboard and no pointer, and without a keyboard no client ever has the
//! focus the clipboard is given to; a RemoteDesktop session on the private bus
//! gives the seat virtual ones. A test whose tools are missing prints why and
//! passes.
//!
//! The teksilo side is the production backend ([`WaylandClipboard`] on a
//! [`WaylandDataHub`]) on a connection whose socket the test thread reads too,
//! dispatching its window's queue, while the hub's thread reads it for the
//! clipboard: the arrangement winit and the hub are in inside an application.
//! The other side is `wl-copy` / `wl-paste`, which on a compositor without
//! data-control use the core protocol too (they map a surface to get the
//! focus), and a raw client of the test's own for the case `wl-copy` cannot
//! play: a source that never writes.

use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_data_device::{self, WlDataDevice};
use wayland_client::protocol::wl_data_device_manager::WlDataDeviceManager;
use wayland_client::protocol::wl_data_offer::WlDataOffer;
use wayland_client::protocol::wl_data_source::{self, WlDataSource};
use wayland_client::protocol::wl_keyboard::{self, WlKeyboard};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::{self, WlSeat};
use wayland_client::protocol::wl_shm::{Format, WlShm};
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};
use wayland_protocols::xdg::shell::client::xdg_surface::{self, XdgSurface};
use wayland_protocols::xdg::shell::client::xdg_toplevel::XdgToplevel;
use wayland_protocols::xdg::shell::client::xdg_wm_base::{self, XdgWmBase};

use super::WaylandClipboard;
use crate::clipboard::ClipboardBackend;
use crate::wayland_data::WaylandDataHub;

static NEXT_DISPLAY: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn which(tool: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|path| std::env::split_paths(&path).any(|dir| dir.join(tool).is_file()))
}

/// Wait for `condition`, calling `between` between tries.
fn wait_for(
    what: &str,
    timeout: Duration,
    mut between: impl FnMut(),
    mut condition: impl FnMut() -> bool,
) {
    let start = Instant::now();
    while !condition() {
        assert!(start.elapsed() < timeout, "timed out waiting for {what}");
        between();
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A private bus and a headless Mutter with virtual input on it.
struct Compositor {
    dbus: Child,
    mutter: Child,
    bus_address: String,
    display: String,
    socket: PathBuf,
    /// Holds the RemoteDesktop session open: Mutter ends it, and removes the
    /// virtual devices, when the client that created it leaves the bus.
    remote: zbus::Connection,
    session: zbus::zvariant::OwnedObjectPath,
}

impl Compositor {
    /// `None`, having said why, when the tools are not installed.
    fn start() -> Option<Self> {
        let missing: Vec<&str> = ["mutter", "dbus-daemon", "wl-copy", "wl-paste"]
            .into_iter()
            .filter(|tool| !which(tool))
            .collect();
        if !missing.is_empty() {
            eprintln!("skipping: not installed: {}", missing.join(", "));
            return None;
        }
        let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) else {
            eprintln!("skipping: XDG_RUNTIME_DIR is not set");
            return None;
        };

        let mut dbus = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("dbus-daemon starts");
        let mut line = String::new();
        std::io::BufRead::read_line(
            &mut std::io::BufReader::new(dbus.stdout.take().expect("piped stdout")),
            &mut line,
        )
        .expect("dbus-daemon prints its address");
        let bus_address = line.trim().to_string();

        let display = format!(
            "teksilo-clipboard-test-{}-{}",
            std::process::id(),
            NEXT_DISPLAY.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let socket = runtime.join(&display);
        let mutter = Command::new("mutter")
            .args([
                "--headless",
                "--wayland",
                "--no-x11",
                "--virtual-monitor",
                "1024x768",
                &format!("--wayland-display={display}"),
            ])
            .env("DBUS_SESSION_BUS_ADDRESS", &bus_address)
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("mutter starts");

        let (remote, session) = async_io::block_on(async {
            let remote = zbus::connection::Builder::address(bus_address.as_str())
                .expect("bus address parses")
                .build()
                .await
                .expect("the private bus answers");
            let start = Instant::now();
            let session: zbus::zvariant::OwnedObjectPath = loop {
                let reply = remote
                    .call_method(
                        Some("org.gnome.Mutter.RemoteDesktop"),
                        "/org/gnome/Mutter/RemoteDesktop",
                        Some("org.gnome.Mutter.RemoteDesktop"),
                        "CreateSession",
                        &(),
                    )
                    .await;
                match reply {
                    Ok(reply) => break reply.body().deserialize().expect("a session path"),
                    Err(err) => {
                        assert!(
                            start.elapsed() < Duration::from_secs(20),
                            "Mutter's RemoteDesktop never appeared: {err}"
                        );
                        async_io::Timer::after(Duration::from_millis(100)).await;
                    }
                }
            };
            remote
                .call_method(
                    Some("org.gnome.Mutter.RemoteDesktop"),
                    &session,
                    Some("org.gnome.Mutter.RemoteDesktop.Session"),
                    "Start",
                    &(),
                )
                .await
                .expect("the RemoteDesktop session starts");
            (remote, session)
        });
        let compositor = Self {
            dbus,
            mutter,
            bus_address,
            display,
            socket,
            remote,
            session,
        };
        // The virtual devices exist once they are first used: a Shift tap
        // gives the seat its keyboard before any client binds it.
        compositor.key(42);
        wait_for(
            "Mutter's socket",
            Duration::from_secs(20),
            || {},
            || compositor.socket.exists(),
        );
        Some(compositor)
    }

    /// Press and release `keycode` (evdev) on the virtual keyboard.
    fn key(&self, keycode: u32) {
        async_io::block_on(async {
            for pressed in [true, false] {
                self.remote
                    .call_method(
                        Some("org.gnome.Mutter.RemoteDesktop"),
                        &self.session,
                        Some("org.gnome.Mutter.RemoteDesktop.Session"),
                        "NotifyKeyboardKeycode",
                        &(keycode, pressed),
                    )
                    .await
                    .expect("Mutter takes the key");
            }
        });
    }

    fn connect(&self) -> Connection {
        let stream = UnixStream::connect(&self.socket).expect("the compositor accepts a client");
        Connection::from_socket(stream).expect("a Wayland connection")
    }

    /// A `wl-clipboard` command on this compositor, with no X server in its
    /// environment.
    fn wl(&self, tool: &str) -> Command {
        let mut command = Command::new(tool);
        command
            .env("WAYLAND_DISPLAY", &self.display)
            .env("DBUS_SESSION_BUS_ADDRESS", &self.bus_address)
            .env_remove("DISPLAY");
        command
    }

    /// `wl-paste` with `args`, its stdout.
    fn paste(&self, args: &[&str]) -> String {
        let out = self
            .wl("wl-paste")
            .args(args)
            .stderr(Stdio::piped())
            .output()
            .expect("wl-paste runs");
        assert!(
            out.status.success(),
            "wl-paste {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).expect("UTF-8 from wl-paste")
    }

    /// `wl-copy` with `args`, `input` on its stdin. It forks to serve the
    /// selection; the server is killed with the compositor.
    fn copy(&self, args: &[&str], input: &str) {
        let mut child = self
            .wl("wl-copy")
            .args(args)
            .stdin(Stdio::piped())
            .spawn()
            .expect("wl-copy runs");
        std::io::Write::write_all(
            &mut child.stdin.take().expect("piped stdin"),
            input.as_bytes(),
        )
        .expect("wl-copy reads its input");
        let status = child.wait().expect("wl-copy finishes");
        assert!(status.success(), "wl-copy {args:?} failed");
    }
}

impl Drop for Compositor {
    fn drop(&mut self) {
        let _ = self.mutter.kill();
        let _ = self.mutter.wait();
        let _ = self.dbus.kill();
        let _ = self.dbus.wait();
    }
}

// ---------------------------------------------------------------------------
// A window, so the compositor has a surface to give the focus to
// ---------------------------------------------------------------------------

#[derive(Default)]
struct WindowState {
    configured: bool,
    /// For the raw client: its keyboard's latest serial and focus.
    serial: u32,
    focused: bool,
    /// For the raw client: the pipes it was asked to fill and holds open.
    held: Vec<OwnedFd>,
}

struct Window {
    queue: EventQueue<WindowState>,
    state: WindowState,
    surface: WlSurface,
    xdg_surface: XdgSurface,
    toplevel: XdgToplevel,
    _buffer: WlBuffer,
    /// The raw client's seat, bound before the window maps so its keyboard
    /// hears the focus arrive.
    seat: Option<WlSeat>,
    globals: wayland_client::globals::GlobalList,
}

impl Window {
    fn open(conn: &Connection, title: &str) -> Self {
        Self::open_with(conn, title, false)
    }

    fn open_with(conn: &Connection, title: &str, with_seat: bool) -> Self {
        let (globals, mut queue) =
            registry_queue_init::<WindowState>(conn).expect("the compositor's globals");
        let qh = queue.handle();
        let seat = with_seat.then(|| {
            globals
                .bind::<WlSeat, _, _>(&qh, 1..=5, ())
                .expect("wl_seat")
        });
        let compositor: WlCompositor = globals.bind(&qh, 4..=5, ()).expect("wl_compositor");
        let shm: WlShm = globals.bind(&qh, 1..=1, ()).expect("wl_shm");
        let wm: XdgWmBase = globals.bind(&qh, 1..=5, ()).expect("xdg_wm_base");
        let surface = compositor.create_surface(&qh, ());
        let xdg_surface = wm.get_xdg_surface(&surface, &qh, ());
        let toplevel = xdg_surface.get_toplevel(&qh, ());
        toplevel.set_title(title.to_string());
        surface.commit();
        let mut state = WindowState::default();
        while !state.configured {
            queue
                .blocking_dispatch(&mut state)
                .expect("configure arrives");
        }
        // A 64×64 opaque buffer: a surface with no content is never mapped,
        // and an unmapped one is never focused.
        let (width, height, stride) = (64i32, 64i32, 256i32);
        let size = (stride * height) as usize;
        // SAFETY: a fresh anonymous file; the name is a valid C string.
        let raw = unsafe { libc::memfd_create(c"teksilo-test-buffer".as_ptr(), libc::MFD_CLOEXEC) };
        assert!(raw >= 0, "memfd_create");
        // SAFETY: `raw` was just created and is owned by nothing else.
        let file = unsafe { <OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(raw) };
        let file = std::fs::File::from(file);
        file.set_len(size as u64).expect("buffer size");
        std::io::Write::write_all(&mut &file, &vec![0x80u8; size]).expect("buffer content");
        let pool = shm.create_pool(file.as_fd(), size as i32, &qh, ());
        let buffer = pool.create_buffer(0, width, height, stride, Format::Argb8888, &qh, ());
        pool.destroy();
        surface.attach(Some(&buffer), 0, 0);
        surface.commit();
        queue.roundtrip(&mut state).expect("the window maps");
        Self {
            queue,
            state,
            surface,
            xdg_surface,
            toplevel,
            _buffer: buffer,
            seat,
            globals,
        }
    }

    /// Answer the compositor (pings, configures) without blocking long.
    fn pump(&mut self) {
        self.queue
            .roundtrip(&mut self.state)
            .expect("the compositor answers");
    }

    fn close(self) {
        self.toplevel.destroy();
        self.xdg_surface.destroy();
        self.surface.destroy();
        let mut queue = self.queue;
        let mut state = self.state;
        let _ = queue.roundtrip(&mut state);
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for WindowState {
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

macro_rules! ignore_events {
    ($($ty:ty),*) => {$(
        impl Dispatch<$ty, ()> for WindowState {
            fn event(_: &mut Self, _: &$ty, _: <$ty as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}
ignore_events!(
    WlCompositor,
    WlShm,
    WlShmPool,
    WlBuffer,
    WlSurface,
    XdgToplevel,
    WlDataDeviceManager,
    WlDataOffer
);

impl Dispatch<XdgWmBase, ()> for WindowState {
    fn event(
        _: &mut Self,
        wm: &XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm.pong(serial);
        }
    }
}

impl Dispatch<XdgSurface, ()> for WindowState {
    fn event(
        state: &mut Self,
        surface: &XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            surface.ack_configure(serial);
            state.configured = true;
        }
    }
}

impl Dispatch<WlSeat, ()> for WindowState {
    fn event(
        _: &mut Self,
        seat: &WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(caps),
        } = event
            && caps.contains(wl_seat::Capability::Keyboard)
        {
            seat.get_keyboard(qh, ());
        }
    }
}

impl Dispatch<WlKeyboard, ()> for WindowState {
    fn event(
        state: &mut Self,
        _: &WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Enter { serial, .. } => {
                state.serial = serial;
                state.focused = true;
            }
            wl_keyboard::Event::Leave { .. } => state.focused = false,
            wl_keyboard::Event::Key { serial, .. } => state.serial = serial,
            _ => {}
        }
    }
}

impl Dispatch<WlDataDevice, ()> for WindowState {
    fn event(
        _: &mut Self,
        _: &WlDataDevice,
        _: wl_data_device::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }

    wayland_client::event_created_child!(WindowState, WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (WlDataOffer, ()),
    ]);
}

impl Dispatch<WlDataSource, ()> for WindowState {
    fn event(
        state: &mut Self,
        _: &WlDataSource,
        event: wl_data_source::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The hung peer: it takes the pipe and never writes or closes it.
        if let wl_data_source::Event::Send { fd, .. } = event {
            state.held.push(fd);
        }
    }
}

// ---------------------------------------------------------------------------
// The teksilo side
// ---------------------------------------------------------------------------

/// The production clipboard with a window to be focused.
struct Teksilo {
    hub: Arc<WaylandDataHub>,
    clipboard: WaylandClipboard,
    window: Window,
}

impl Teksilo {
    fn open(compositor: &Compositor) -> Self {
        let conn = compositor.connect();
        let hub = WaylandDataHub::for_connection(conn.clone()).expect("the hub starts");
        let clipboard = WaylandClipboard::on_hub(hub.clone());
        let window = Window::open(&conn, "teksilo");
        let mut teksilo = Self {
            hub,
            clipboard,
            window,
        };
        teksilo.wait_for_focus();
        teksilo
    }

    fn wait_for_focus(&mut self) {
        let hub = self.hub.clone();
        let window = &mut self.window;
        wait_for(
            "the teksilo window to have keyboard focus",
            Duration::from_secs(10),
            || window.pump(),
            || {
                hub.with_state(|state| state.has_keyboard_focus())
                    .expect("the hub answers")
            },
        );
    }
}

impl Drop for Teksilo {
    fn drop(&mut self) {
        self.hub.shutdown();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
#[ignore = "needs mutter, dbus-daemon and wl-clipboard; run with --ignored"]
fn text_another_client_copied_pastes_into_teksilo() {
    let Some(compositor) = Compositor::start() else {
        return;
    };
    let mut teksilo = Teksilo::open(&compositor);
    // wl-copy maps a surface of its own to be given the focus, sets the
    // selection, and leaves; the focus, and with it the selection, come back
    // to the teksilo window.
    compositor.copy(&[], "Bonjour, ☃, copié ailleurs");
    let mut pasted = None;
    wait_for(
        "the copied text to reach teksilo",
        Duration::from_secs(10),
        || teksilo.window.pump(),
        || {
            pasted = teksilo.clipboard.get_text().ok();
            pasted.as_deref() == Some("Bonjour, ☃, copié ailleurs")
        },
    );
    assert!(teksilo.clipboard.has_text());
}

#[test]
#[ignore = "needs mutter, dbus-daemon and wl-clipboard; run with --ignored"]
fn html_another_client_copied_pastes_into_teksilo() {
    let Some(compositor) = Compositor::start() else {
        return;
    };
    let mut teksilo = Teksilo::open(&compositor);
    compositor.copy(&["--type", "text/html"], "<p><b>gras</b> et é</p>");
    wait_for(
        "the copied HTML to reach teksilo",
        Duration::from_secs(10),
        || teksilo.window.pump(),
        || teksilo.clipboard.has_html(),
    );
    assert_eq!(
        teksilo.clipboard.get_html().as_deref(),
        Ok("<p><b>gras</b> et é</p>")
    );
}

#[test]
#[ignore = "needs mutter, dbus-daemon and wl-clipboard; run with --ignored"]
fn what_teksilo_copied_pastes_elsewhere_as_text_and_html() {
    let Some(compositor) = Compositor::start() else {
        return;
    };
    let mut teksilo = Teksilo::open(&compositor);
    teksilo
        .clipboard
        .set_html("<p>riche, é ✓</p>", "riche, é ✓")
        .expect("a focused window can copy");
    teksilo.window.pump();

    // Each wl-paste maps a surface of its own and takes the focus from the
    // teksilo window to read the selection, so these also show the selection
    // served while teksilo is not focused, and served by the hub's thread
    // alone: the test thread, which owns the window, is blocked in `wl-paste`
    // and dispatches nothing meanwhile.
    let types = compositor.paste(&["--list-types"]);
    for mime in [
        "text/html",
        "text/plain;charset=utf-8",
        "UTF8_STRING",
        "text/plain",
        "TEXT",
        "STRING",
    ] {
        assert!(
            types.lines().any(|l| l == mime),
            "{mime} not offered: {types}"
        );
    }
    assert_eq!(compositor.paste(&["--no-newline"]), "riche, é ✓");
    let html = compositor.paste(&["--no-newline", "--type", "text/html"]);
    assert!(html.ends_with("<p>riche, é ✓</p>"), "{html}");
    assert!(html.contains("charset=utf-8"), "{html}");

    // And teksilo still reads its own copy, exactly as it was given.
    teksilo.wait_for_focus();
    assert_eq!(teksilo.clipboard.get_text().as_deref(), Ok("riche, é ✓"));
    assert_eq!(
        teksilo.clipboard.get_html().as_deref(),
        Ok("<p>riche, é ✓</p>")
    );
}

#[test]
#[ignore = "needs mutter, dbus-daemon and wl-clipboard; run with --ignored"]
fn a_copy_survives_the_window_that_made_it() {
    let Some(compositor) = Compositor::start() else {
        return;
    };
    let conn = compositor.connect();
    let hub = WaylandDataHub::for_connection(conn.clone()).expect("the hub starts");
    let mut clipboard = WaylandClipboard::on_hub(hub.clone());
    let mut first = Window::open(&conn, "first");
    let mut second = Window::open(&conn, "second");
    wait_for(
        "focus",
        Duration::from_secs(10),
        || {
            first.pump();
            second.pump();
        },
        || hub.with_state(|s| s.has_keyboard_focus()).unwrap_or(false),
    );
    clipboard
        .set_text("kept after its window closed")
        .expect("a focused window can copy");
    second.pump();
    second.close();
    first.pump();
    assert_eq!(
        compositor.paste(&["--no-newline"]),
        "kept after its window closed"
    );
    hub.shutdown();
}

#[test]
#[ignore = "needs mutter, dbus-daemon and wl-clipboard; run with --ignored"]
fn another_clients_copy_replaces_ours() {
    let Some(compositor) = Compositor::start() else {
        return;
    };
    let mut teksilo = Teksilo::open(&compositor);
    teksilo
        .clipboard
        .set_text("ours")
        .expect("a focused window can copy");
    teksilo.window.pump();
    // Our source is cancelled as wl-copy's takes its place; a paste in teksilo
    // must then read the other client's text, not ours from memory.
    compositor.copy(&[], "theirs");
    wait_for(
        "the other client's copy to replace ours",
        Duration::from_secs(10),
        || teksilo.window.pump(),
        || teksilo.clipboard.get_text().ok().as_deref() == Some("theirs"),
    );
}

#[test]
#[ignore = "needs mutter, dbus-daemon and wl-clipboard; run with --ignored"]
fn a_copy_the_compositor_ignores_leaves_the_previous_one() {
    let Some(compositor) = Compositor::start() else {
        return;
    };
    let mut teksilo = Teksilo::open(&compositor);
    // Two copies with no input between them carry the same serial, and Mutter
    // ignores a `set_selection` whose serial is no newer than the selection's
    // (`selection_serial - serial < UINT32_MAX / 2`), without a word. The
    // first copy must survive the second's refusal, for teksilo and for
    // everyone else alike.
    teksilo
        .clipboard
        .set_text("first")
        .expect("a focused window can copy");
    teksilo
        .clipboard
        .set_text("second")
        .expect("the request goes out; the compositor answers later");
    wait_for(
        "the ignored copy to be dropped",
        Duration::from_secs(10),
        || teksilo.window.pump(),
        || teksilo.clipboard.get_text().ok().as_deref() == Some("first"),
    );
    assert_eq!(compositor.paste(&["--no-newline"]), "first");
}

#[test]
#[ignore = "needs mutter, dbus-daemon and wl-clipboard; run with --ignored"]
fn a_copy_refused_for_want_of_focus_leaves_the_previous_one() {
    let Some(compositor) = Compositor::start() else {
        return;
    };
    let conn = compositor.connect();
    let hub = WaylandDataHub::for_connection(conn.clone()).expect("the hub starts");
    let mut clipboard = WaylandClipboard::on_hub(hub.clone());
    let mut window = Window::open(&conn, "only");
    wait_for(
        "focus",
        Duration::from_secs(10),
        || window.pump(),
        || hub.with_state(|s| s.has_keyboard_focus()).unwrap_or(false),
    );
    clipboard
        .set_text("kept")
        .expect("a focused window can copy");
    window.pump();
    // With its only window gone the process has no focus, so the next copy is
    // refused; the one before it must stay the clipboard.
    window.close();
    wait_for(
        "the focus to leave",
        Duration::from_secs(10),
        || {},
        || !hub.with_state(|s| s.has_keyboard_focus()).unwrap_or(true),
    );
    let err = clipboard.set_text("refused").unwrap_err();
    assert!(err.contains("keyboard focus"), "{err}");
    assert_eq!(compositor.paste(&["--no-newline"]), "kept");
    hub.shutdown();
}

#[test]
#[ignore = "needs mutter, dbus-daemon and wl-clipboard; run with --ignored"]
fn copying_without_focus_is_refused_not_lost() {
    let Some(compositor) = Compositor::start() else {
        return;
    };
    let conn = compositor.connect();
    let hub = WaylandDataHub::for_connection(conn).expect("the hub starts");
    let mut clipboard = WaylandClipboard::on_hub(hub.clone());
    // No window: nothing of ours can have the focus.
    let err = clipboard.set_text("nowhere").unwrap_err();
    assert!(
        err.contains("keyboard focus") || err.contains("no input"),
        "{err}"
    );
    hub.shutdown();
}

#[test]
#[ignore = "needs mutter, dbus-daemon and wl-clipboard; run with --ignored"]
fn a_peer_that_never_writes_cannot_freeze_a_paste() {
    let Some(compositor) = Compositor::start() else {
        return;
    };
    let mut teksilo = Teksilo::open(&compositor);

    // A raw client offers text and, asked for it, holds the pipe open forever.
    let peer_conn = compositor.connect();
    let mut peer = Window::open_with(&peer_conn, "hung peer", true);
    let qh = peer.queue.handle();
    let seat = peer.seat.clone().expect("the peer's seat");
    let manager: WlDataDeviceManager = peer.globals.bind(&qh, 1..=3, ()).expect("manager");
    let device = manager.get_data_device(&seat, &qh, ());
    let started = Instant::now();
    while !peer.state.focused {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the peer never had the focus"
        );
        peer.pump();
        std::thread::sleep(Duration::from_millis(20));
    }
    let source = manager.create_data_source(&qh, ());
    source.offer("text/plain;charset=utf-8".into());
    device.set_selection(Some(&source), peer.state.serial);
    peer.pump();
    // The focus goes back to teksilo, which is then given the peer's offer.
    peer.close();
    teksilo.wait_for_focus();
    wait_for(
        "the peer's offer",
        Duration::from_secs(10),
        || teksilo.window.pump(),
        || teksilo.clipboard.has_text(),
    );

    // The peer's queue is never dispatched again: it neither writes nor
    // closes. The paste gives up within its limits instead of hanging.
    let started = Instant::now();
    let err = teksilo.clipboard.get_text().unwrap_err();
    let took = started.elapsed();
    assert!(
        err.contains("sent nothing") || err.contains("took longer"),
        "{err}"
    );
    assert!(took < Duration::from_secs(6), "the paste took {took:?}");

    // A rich paste asks again at once (HTML, then text): the same silent owner
    // is not waited for a second time.
    let started = Instant::now();
    let err = teksilo.clipboard.get_text().unwrap_err();
    let took = started.elapsed();
    assert!(err.contains("did not answer"), "{err}");
    assert!(took < Duration::from_millis(500), "the retry took {took:?}");
    drop(peer_conn);
}
