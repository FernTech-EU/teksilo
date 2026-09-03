// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Local IPC for the debug-only automation bridge.
//!
//! One trait pair, two OS backends — the same shape as
//! [`external_dnd`](crate::external_dnd) and
//! [`native_menu`](crate::native_menu):
//!
//! - **Unix** ([`unix`]) — a Unix-domain socket in a `0700` per-process
//!   directory, the socket itself `0600`.
//! - **Windows** ([`windows`]) — a named pipe under `\\.\pipe\`, with an
//!   explicit DACL granting only the creating user.
//!
//! Only `bind` / `accept` / `connect` and their access control live here. The
//! framing, the token handshake and the endpoint descriptor are identical
//! everywhere and live in [`teksilo_automation::wire`].
//!
//! ## Why this is not "just a socket"
//!
//! The security posture is the whole point, and each OS spells it differently.
//! On Unix the kernel checks the socket's mode against the connecting uid. On
//! Windows a named pipe created with a null security descriptor grants *read
//! access to Everyone and to the anonymous account* — so the descriptor must be
//! built explicitly, or the bridge would be reachable by any local user with
//! nothing but the token in the way. Both backends therefore refuse to bind at
//! all rather than fall back to a weaker mode.

use std::io::{self, Read, Write};
use std::time::Duration;

use teksilo_automation::wire::{Endpoint, Transport};

#[cfg(unix)]
pub mod unix;
#[cfg(windows)]
pub mod windows;

/// A bidirectional byte stream to one automation peer.
///
/// Deliberately just `Read + Write`: the wire protocol reads a token line and
/// then alternates frames on a single connection, so nothing needs a second
/// handle, and no backend has to provide a `try_clone` (which a Windows pipe
/// has no clean equivalent for).
pub trait TransportStream: Read + Write + Send {
    /// Set a deadline for subsequent reads; `None` blocks indefinitely.
    ///
    /// A read that hits the deadline fails with [`io::ErrorKind::TimedOut`] and
    /// leaves the stream usable — the connection is not torn down.
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()>;
}

/// A bound endpoint that accepts one peer at a time.
///
/// Dropping it releases the OS resource: on Unix that unlinks the socket and
/// its per-process directory, on Windows it closes the pipe instance.
pub trait TransportListener: Send {
    /// Block until a peer connects.
    fn accept(&mut self) -> io::Result<Box<dyn TransportStream>>;
}

/// The result of [`bind`]: something to accept on, and the address to publish.
pub struct BoundTransport {
    /// Accepts peers.
    pub listener: Box<dyn TransportListener>,
    /// The address to write into the endpoint descriptor.
    pub endpoint: Endpoint,
}

impl std::fmt::Debug for BoundTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoundTransport")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

/// Bind this platform's automation endpoint for process `pid`.
///
/// Fails rather than degrading: a host that cannot provide a private endpoint
/// must not get a public one.
pub fn bind(pid: u32) -> io::Result<BoundTransport> {
    #[cfg(unix)]
    {
        unix::bind(pid)
    }
    #[cfg(windows)]
    {
        windows::bind(pid)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "the automation bridge has no transport on this platform",
        ))
    }
}

/// Is a bridge actually listening at `endpoint`?
///
/// A descriptor outlives its process whenever the app exits without unwinding
/// — `std::process::exit`, a panic-abort, a crash, a kill — so "the file is
/// there" does not mean "someone is listening". This opens a connection and
/// drops it immediately; the bridge sees a peer that says nothing, fails the
/// token handshake on EOF, and goes back to accepting. Microseconds of slot
/// occupancy in exchange for never offering a dead endpoint to a caller.
pub fn probe(endpoint: &Endpoint) -> bool {
    connect(endpoint).is_ok()
}

/// Connect to a bridge published at `endpoint`.
pub fn connect(endpoint: &Endpoint) -> io::Result<Box<dyn TransportStream>> {
    match endpoint.transport {
        #[cfg(unix)]
        Transport::Unix => unix::connect(&endpoint.address),
        #[cfg(windows)]
        Transport::NamedPipe => windows::connect(&endpoint.address),
        other => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("this build cannot open a {other:?} endpoint"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bind, connect, and push bytes both ways — on whatever transport this
    /// platform actually uses. The one test that proves the Windows named-pipe
    /// backend and the Unix socket backend are interchangeable.
    #[test]
    fn round_trips_on_the_native_transport() {
        let pid = std::process::id();
        let mut bound = match bind(pid) {
            Ok(b) => b,
            // A sandboxed CI container can legitimately refuse to bind; that is
            // not a failure of this code.
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => return,
            Err(e) => panic!("bind failed: {e}"),
        };
        assert_eq!(bound.endpoint.transport, Transport::native());
        let endpoint = bound.endpoint.clone();

        let server = std::thread::spawn(move || {
            let mut peer = bound.listener.accept().expect("accept");
            let mut got = [0u8; 5];
            peer.read_exact(&mut got).expect("server read");
            peer.write_all(b"pong!").expect("server write");
            peer.flush().ok();
            got
        });

        let mut client = connect(&endpoint).expect("connect");
        client.write_all(b"ping!").expect("client write");
        client.flush().expect("client flush");
        let mut back = [0u8; 5];
        client.read_exact(&mut back).expect("client read");

        assert_eq!(&back, b"pong!");
        assert_eq!(&server.join().unwrap(), b"ping!");
    }

    /// A read deadline must fire as a typed timeout and leave the stream alive.
    #[test]
    fn read_timeout_is_reported_not_fatal() {
        let pid = std::process::id().wrapping_add(1);
        let mut bound = match bind(pid) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => return,
            Err(e) => panic!("bind failed: {e}"),
        };
        let endpoint = bound.endpoint.clone();

        let server = std::thread::spawn(move || {
            let mut peer = bound.listener.accept().expect("accept");
            peer.set_read_timeout(Some(Duration::from_millis(80)))
                .expect("set timeout");
            let mut buf = [0u8; 1];
            let err = peer.read_exact(&mut buf).unwrap_err();
            assert_eq!(
                err.kind(),
                io::ErrorKind::TimedOut,
                "a silent peer must time out, not block forever: {err}"
            );
            // Still usable afterwards: clear the deadline and read for real.
            peer.set_read_timeout(None).expect("clear timeout");
            peer.read_exact(&mut buf).expect("read after timeout");
            buf[0]
        });

        let mut client = connect(&endpoint).expect("connect");
        // Say nothing for longer than the server's deadline, then speak.
        std::thread::sleep(Duration::from_millis(250));
        client.write_all(b"z").expect("client write");
        client.flush().ok();

        assert_eq!(server.join().unwrap(), b'z');
    }
}
