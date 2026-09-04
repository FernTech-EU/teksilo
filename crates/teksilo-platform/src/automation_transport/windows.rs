// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Named-pipe transport for Windows.
//!
//! Written against the `windows` crate 0.62 API and exercised on a Windows
//! host; it is `cfg(target_os = "windows")`, so it is not built on a Linux or
//! macOS development machine — re-verify on Windows after any change here.
//!
//! ## Security
//!
//! `CreateNamedPipeW` with a null `lpSecurityAttributes` is **not** a neutral
//! default. Microsoft's own wording: the default ACLs "grant full control to
//! the LocalSystem account, administrators, and the creator owner. They also
//! grant read access to members of the Everyone group and the anonymous
//! account." For a bridge that drives the user's UI that is unacceptable, so
//! the descriptor is always explicit: the current process token's user SID,
//! and nobody else, in a *protected* DACL (`D:P`, so nothing is inherited in).
//!
//! `PIPE_REJECT_REMOTE_CLIENTS` is set as a second layer — it blocks the SMB
//! path only, and does **not** keep out other local users or other
//! terminal-services sessions, so it is never a substitute for the DACL. It is
//! set once, inside the single `dwPipeMode` bitmask, rather than through any
//! after-the-fact call: tokio shipped an advisory (GHSA-7rrj-xr53-82p7) where a
//! later builder call silently cleared exactly this flag.
//!
//! ## Why overlapped I/O
//!
//! A byte-mode pipe in `PIPE_WAIT` blocks forever, and there is no
//! `SO_RCVTIMEO` equivalent for pipes. The token handshake needs a deadline —
//! otherwise a peer that connects and says nothing holds the single connection
//! slot for the life of the process — so every handle is opened
//! `FILE_FLAG_OVERLAPPED` and reads wait on an event with a timeout,
//! cancelling the pending operation with `CancelIoEx` if it expires.

use std::io;
use std::sync::Mutex;
use std::time::Duration;

use teksilo_automation::wire::Endpoint;
use windows::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_NO_DATA, ERROR_PIPE_BUSY,
    ERROR_PIPE_CONNECTED, ERROR_PIPE_NOT_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE, HLOCAL,
    LocalFree, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_SHARE_NONE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
    ReadFile, WriteFile,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_WAIT, WaitNamedPipeW,
};
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, INFINITE, OpenProcessToken, ResetEvent, WaitForSingleObject,
};
use windows::core::{PCWSTR, PWSTR};

use super::{BoundTransport, TransportListener, TransportStream};

/// Pipe buffer hint. Frames are read in chunks anyway; this only sizes the
/// kernel's staging buffers.
const PIPE_BUFFER: u32 = 64 * 1024;

/// The pipe name for a given process.
pub(super) fn pipe_name(pid: u32) -> String {
    format!(r"\\.\pipe\teksilo-automation-{pid}")
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn last_io_error(context: &str) -> io::Error {
    io::Error::other(format!("{context}: {}", io::Error::last_os_error()))
}

// ---------------------------------------------------------------------------
// Security descriptor
// ---------------------------------------------------------------------------

/// An owner-only security descriptor, freed on drop.
struct OwnerOnlySecurity {
    descriptor: PSECURITY_DESCRIPTOR,
}

impl OwnerOnlySecurity {
    /// Build `D:P(A;;GA;;;<current user SID>)`.
    ///
    /// `GA` (generic all) rather than a narrower mask because the only
    /// principal on the ACL is the creating user, and creating a *second*
    /// instance of the pipe needs `FILE_CREATE_PIPE_INSTANCE` — a narrower
    /// grant would lock the server out of its own next `accept`.
    fn current_user() -> io::Result<Self> {
        let sid = current_user_sid_string()?;
        let sddl = wide(&format!("D:P(A;;GA;;;{sid})"));
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: `sddl` is a NUL-terminated UTF-16 buffer that outlives the
        // call; the descriptor is an out-parameter we own and free on drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(sddl.as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
        }
        .map_err(|e| io::Error::other(format!("building the pipe security descriptor: {e}")))?;
        Ok(Self { descriptor })
    }

    fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.descriptor.0,
            bInheritHandle: false.into(),
        }
    }
}

impl Drop for OwnerOnlySecurity {
    fn drop(&mut self) {
        if !self.descriptor.is_invalid() {
            // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.descriptor.0)));
            }
        }
    }
}

/// The current process token's user SID, in `S-1-5-…` string form.
fn current_user_sid_string() -> io::Result<String> {
    // SAFETY: every pointer below is either a stack out-parameter or a slice we
    // own; `token` is closed before returning on every path.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
            .map_err(|e| io::Error::other(format!("opening the process token: {e}")))?;
        let _guard = HandleGuard(token);

        // Size probe: this call is expected to fail with ERROR_INSUFFICIENT_BUFFER.
        let mut needed = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut needed);
        if needed == 0 {
            return Err(last_io_error("sizing the token user information"));
        }
        let mut buf = vec![0u8; needed as usize];
        GetTokenInformation(
            token,
            TokenUser,
            Some(buf.as_mut_ptr().cast()),
            needed,
            &mut needed,
        )
        .map_err(|e| io::Error::other(format!("reading the token user information: {e}")))?;

        let token_user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut sid_str = PWSTR::null();
        ConvertSidToStringSidW(token_user.User.Sid, &mut sid_str)
            .map_err(|e| io::Error::other(format!("formatting the user SID: {e}")))?;
        let owned = sid_str.to_string().unwrap_or_default();
        let _ = LocalFree(Some(HLOCAL(sid_str.0.cast())));
        if owned.is_empty() {
            return Err(io::Error::other(
                "the user SID formatted to an empty string",
            ));
        }
        Ok(owned)
    }
}

/// Closes a HANDLE on drop.
struct HandleGuard(HANDLE);

impl Drop for HandleGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            // SAFETY: we own this handle.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Stream
// ---------------------------------------------------------------------------

/// One end of a connected pipe, doing overlapped I/O with an optional read
/// deadline.
pub struct PipeStream {
    handle: HANDLE,
    /// Manual-reset event used by every overlapped operation on this handle.
    /// Operations are strictly sequential (the protocol is single-in-flight),
    /// so one event is enough.
    event: HANDLE,
    read_timeout: Mutex<Option<Duration>>,
}

// SAFETY: `PipeStream` owns both handles exclusively and never shares them; the
// `windows` crate's HANDLE is `!Send` only because it is a raw pointer newtype.
unsafe impl Send for PipeStream {}

impl PipeStream {
    fn new(handle: HANDLE) -> io::Result<Self> {
        // SAFETY: a manual-reset, initially-unsignalled, unnamed event.
        let event = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }
            .map_err(|e| io::Error::other(format!("creating the pipe I/O event: {e}")))?;
        Ok(Self {
            handle,
            event,
            read_timeout: Mutex::new(None),
        })
    }

    /// Wait for a pending overlapped operation, honouring `timeout`.
    ///
    /// On timeout the operation is cancelled and then *waited for* — an
    /// `OVERLAPPED` must not be reused or dropped while the kernel still owns
    /// it, and `CancelIoEx` only requests cancellation.
    fn await_overlapped(&self, ov: &mut OVERLAPPED, timeout: Option<Duration>) -> io::Result<u32> {
        let millis = timeout
            .map(|d| u32::try_from(d.as_millis()).unwrap_or(u32::MAX - 1))
            .unwrap_or(INFINITE);
        // SAFETY: `self.event` is the event this OVERLAPPED was armed with.
        let waited = unsafe { WaitForSingleObject(self.event, millis) };
        if waited == WAIT_TIMEOUT {
            // Cancellation races completion: a byte-mode pipe read can already
            // have moved data into the caller's buffer by the time `CancelIoEx`
            // lands. Those bytes are *gone from the pipe*, so throwing the
            // count away would silently lose them — and the trait promises a
            // timeout "leaves the stream usable". Report the partial transfer
            // as a short read instead; only a genuinely empty cancellation is a
            // timeout.
            let mut transferred = 0u32;
            // SAFETY: cancels only the operation described by `ov` on our handle,
            // then waits for the kernel to release `ov` before it is dropped.
            let completed = unsafe {
                let _ = CancelIoEx(self.handle, Some(ov as *const OVERLAPPED));
                GetOverlappedResult(self.handle, ov, &mut transferred, true)
            };
            if completed.is_ok() && transferred > 0 {
                return Ok(transferred);
            }
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the pipe read deadline expired",
            ));
        }
        if waited != WAIT_OBJECT_0 {
            return Err(last_io_error("waiting on the pipe I/O event"));
        }
        let mut transferred = 0u32;
        // SAFETY: the operation has completed; `ov` is still valid.
        unsafe { GetOverlappedResult(self.handle, ov, &mut transferred, false) }.map_err(|e| {
            if e.code() == ERROR_BROKEN_PIPE.into() || e.code() == ERROR_PIPE_NOT_CONNECTED.into() {
                io::Error::from(io::ErrorKind::UnexpectedEof)
            } else {
                io::Error::other(format!("completing the pipe operation: {e}"))
            }
        })?;
        Ok(transferred)
    }

    fn armed_overlapped(&self) -> io::Result<OVERLAPPED> {
        // SAFETY: our own manual-reset event.
        unsafe { ResetEvent(self.event) }
            .map_err(|e| io::Error::other(format!("resetting the pipe I/O event: {e}")))?;
        Ok(OVERLAPPED {
            hEvent: self.event,
            ..Default::default()
        })
    }
}

impl Drop for PipeStream {
    fn drop(&mut self) {
        // SAFETY: both handles are ours.
        unsafe {
            let _ = CloseHandle(self.handle);
            let _ = CloseHandle(self.event);
        }
    }
}

impl io::Read for PipeStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut ov = self.armed_overlapped()?;
        let mut read = 0u32;
        // SAFETY: `buf` and `ov` outlive the call; on ERROR_IO_PENDING the
        // kernel keeps writing into them until `await_overlapped` returns, and
        // that function never leaves an operation outstanding.
        let started = unsafe {
            ReadFile(
                self.handle,
                Some(buf),
                Some(&mut read),
                Some(&mut ov as *mut OVERLAPPED),
            )
        };
        match started {
            Ok(()) => {
                // Completed inline; still drain the event so the next
                // operation starts from a clean state.
                let n = self.await_overlapped(&mut ov, None)?;
                Ok(n as usize)
            }
            Err(e) if e.code() == ERROR_IO_PENDING.into() => {
                let timeout = *self.read_timeout.lock().unwrap();
                match self.await_overlapped(&mut ov, timeout) {
                    Ok(n) => Ok(n as usize),
                    // A closed peer is end-of-stream, not an error: the frame
                    // loop reads it as "the client disconnected".
                    Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Ok(0),
                    Err(e) => Err(e),
                }
            }
            Err(e)
                if e.code() == ERROR_BROKEN_PIPE.into()
                    || e.code() == ERROR_PIPE_NOT_CONNECTED.into() =>
            {
                Ok(0)
            }
            Err(e) => Err(io::Error::other(format!("reading from the pipe: {e}"))),
        }
    }
}

impl io::Write for PipeStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut ov = self.armed_overlapped()?;
        let mut written = 0u32;
        // SAFETY: as for `read` — the buffer outlives the completed operation.
        let started = unsafe {
            WriteFile(
                self.handle,
                Some(buf),
                Some(&mut written),
                Some(&mut ov as *mut OVERLAPPED),
            )
        };
        // Writes are never given a deadline: a partial frame would desync the
        // peer, and a reply must land whole or not at all.
        match started {
            Ok(()) => Ok(self.await_overlapped(&mut ov, None)? as usize),
            Err(e) if e.code() == ERROR_IO_PENDING.into() => {
                Ok(self.await_overlapped(&mut ov, None)? as usize)
            }
            Err(e) => Err(io::Error::other(format!("writing to the pipe: {e}"))),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        // Every write above is already completed synchronously before it
        // returns, so there is nothing buffered on this side to push.
        Ok(())
    }
}

impl TransportStream for PipeStream {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        *self.read_timeout.lock().unwrap() = timeout;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Listener
// ---------------------------------------------------------------------------

struct PipeListener {
    name: Vec<u16>,
    security: OwnerOnlySecurity,
    /// The instance created ahead of the next `accept`, so the pipe exists —
    /// and is therefore connectable — from the moment `bind` returns.
    pending: Option<HANDLE>,
}

// SAFETY: the pending handle is owned exclusively by this listener.
unsafe impl Send for PipeListener {}

impl PipeListener {
    fn create_instance(&self) -> io::Result<HANDLE> {
        let attrs = self.security.attributes();
        // SAFETY: `name` is NUL-terminated and outlives the call; `attrs`
        // borrows a descriptor owned by `self.security`.
        let handle = unsafe {
            CreateNamedPipeW(
                PCWSTR(self.name.as_ptr()),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
                // One bitmask, set once — see the module docs on GHSA-7rrj-xr53-82p7.
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                // Two instances: one being served, one listening. The bridge
                // still serves strictly one client at a time — the accept loop
                // is serial — so this is the Windows spelling of the backlog a
                // Unix listener gets from the kernel, not a second session.
                //
                // It cannot be 1. `accept` creates the next instance eagerly so
                // that a client arriving immediately after the previous one
                // disconnects still finds something listening; at a maximum of
                // one that creation always fails with ERROR_PIPE_BUSY, leaving a
                // window with no listening instance. Back-to-back attaches then
                // fail — which is exactly what `--list` followed by `--attach`
                // does.
                2,
                PIPE_BUFFER,
                PIPE_BUFFER,
                0,
                Some(&attrs),
            )
        };
        if handle.is_invalid() {
            return Err(last_io_error("creating the named pipe"));
        }
        Ok(handle)
    }
}

impl TransportListener for PipeListener {
    fn accept(&mut self) -> io::Result<Box<dyn TransportStream>> {
        // A pipe instance can go stale between being created and being
        // connected: a client may open it and drop straight away — which is
        // exactly what a `--list` probe does — leaving the instance in the
        // "closing" state that `ConnectNamedPipe` reports as ERROR_NO_DATA.
        // That is a used-up instance, not a broken listener, so recycle it and
        // try a fresh one. Returning the error instead would end the accept
        // loop and take the whole bridge down with it for the rest of the
        // process's life.
        const RECYCLE_ATTEMPTS: usize = 8;

        for _ in 0..RECYCLE_ATTEMPTS {
            let handle = match self.pending.take() {
                Some(h) => h,
                None => self.create_instance()?,
            };
            // `PipeStream` owns the handle from here on; until it exists, a
            // failure would leak the instance we just took out of `pending`.
            let stream = match PipeStream::new(handle) {
                Ok(s) => s,
                Err(e) => {
                    // SAFETY: ours, never handed out.
                    unsafe {
                        let _ = CloseHandle(handle);
                    }
                    return Err(e);
                }
            };
            let mut ov = stream.armed_overlapped()?;

            // SAFETY: `ov` lives until `await_overlapped` returns, and that
            // function never leaves the operation outstanding.
            let started = unsafe { ConnectNamedPipe(handle, Some(&mut ov as *mut OVERLAPPED)) };
            let outcome: Result<(), AcceptFailure> = match started {
                Ok(()) => Ok(()),
                // The documented race: a client connected between
                // `CreateNamedPipeW` and `ConnectNamedPipe`. Microsoft: "there
                // is a good connection between client and server, even though
                // the function returns zero."
                Err(e) if e.code() == ERROR_PIPE_CONNECTED.into() => Ok(()),
                Err(e) if e.code() == ERROR_IO_PENDING.into() => stream
                    .await_overlapped(&mut ov, None)
                    .map(|_| ())
                    .map_err(|err| AcceptFailure {
                        // `await_overlapped` normalises a broken /
                        // not-connected pipe to end-of-stream: the same used-up
                        // instance, just noticed later.
                        stale: err.kind() == io::ErrorKind::UnexpectedEof,
                        err,
                    }),
                Err(e) => Err(AcceptFailure {
                    stale: is_stale_instance(&e),
                    err: io::Error::other(format!("accepting on the pipe: {e}")),
                }),
            };

            match outcome {
                Ok(()) => {
                    // Have the next instance listening *before* handing this one
                    // back, so a client arriving the instant this connection ends
                    // still finds somewhere to go. This is why the pipe allows two
                    // instances: at one, this creation always fails and leaves a
                    // gap with nothing listening.
                    self.pending = self.create_instance().ok();
                    return Ok(Box::new(stream));
                }
                Err(f) if f.stale => {
                    // Drop `stream` (closing the used-up instance) and loop.
                    drop(stream);
                    continue;
                }
                Err(f) => return Err(f.err),
            }
        }
        Err(io::Error::other(
            "the named pipe kept yielding stale instances; giving up on this accept",
        ))
    }
}

impl Drop for PipeListener {
    fn drop(&mut self) {
        if let Some(h) = self.pending.take() {
            // SAFETY: ours, never handed out.
            unsafe {
                let _ = CloseHandle(h);
            }
        }
    }
}

/// A failed `accept`, plus whether the *instance* was merely used up.
///
/// The classification is carried alongside the error rather than recovered
/// from it afterwards: `io::Error::other` discards `raw_os_error()`, so a
/// downstream classifier would be reduced to substring-matching a formatted
/// `windows::core::Error` — a match that breaks silently when that crate
/// changes its `Display`, and whose failure mode here is the accept loop
/// ending and the live bridge dying for the rest of the process's life.
struct AcceptFailure {
    /// The instance was used up, not the pipe broken: retry with a fresh one.
    stale: bool,
    err: io::Error,
}

/// Does this error mean "that instance was used up", rather than "the pipe is
/// broken"? A client that connects and disconnects before the server calls
/// `ConnectNamedPipe` leaves the instance in a closing state; the fix is a
/// fresh instance, not a failed accept.
fn is_stale_instance(e: &windows::core::Error) -> bool {
    let code = e.code();
    code == ERROR_NO_DATA.into()
        || code == ERROR_BROKEN_PIPE.into()
        || code == ERROR_PIPE_NOT_CONNECTED.into()
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

pub(super) fn bind(pid: u32) -> io::Result<BoundTransport> {
    let name = pipe_name(pid);
    let security = OwnerOnlySecurity::current_user()?;
    let mut listener = PipeListener {
        name: wide(&name),
        security,
        pending: None,
    };
    // Create the first instance eagerly: the endpoint descriptor is published
    // right after `bind` returns, and a client acts on it immediately.
    listener.pending = Some(listener.create_instance()?);
    Ok(BoundTransport {
        listener: Box::new(listener),
        endpoint: Endpoint::named_pipe(name),
    })
}

pub(super) fn connect(address: &str) -> io::Result<Box<dyn TransportStream>> {
    connect_within(address, CONNECT_PATIENCE)
}

/// How long `connect` keeps trying before giving up.
///
/// A named-pipe instance is unavailable during two ordinary, brief windows: the
/// server is between `accept`s, or the previous client has disconnected and the
/// server has not yet released the instance. Neither means "no bridge here", so
/// a single attempt turns a routine race into a hard failure — which is what
/// made `--list` (which probes by connecting and dropping) followed by
/// `--attach` fail outright.
const CONNECT_PATIENCE: Duration = Duration::from_secs(5);

pub(super) fn connect_within(
    address: &str,
    patience: Duration,
) -> io::Result<Box<dyn TransportStream>> {
    let name = wide(address);
    let deadline = std::time::Instant::now() + patience;
    let mut last_busy = false;

    loop {
        // SAFETY: `name` is NUL-terminated and outlives the call.
        let handle = unsafe {
            CreateFileW(
                PCWSTR(name.as_ptr()),
                (GENERIC_READ | GENERIC_WRITE).0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                None,
            )
        };
        match handle {
            Ok(h) if !h.is_invalid() => {
                return match PipeStream::new(h) {
                    Ok(s) => Ok(Box::new(s)),
                    Err(e) => {
                        // SAFETY: ours, and nothing else has seen it.
                        unsafe {
                            let _ = CloseHandle(h);
                        }
                        Err(e)
                    }
                };
            }
            Ok(_) => return Err(last_io_error("opening the named pipe")),
            // Busy: an instance exists but is taken. Wait for one to free up —
            // `WaitNamedPipeW` is the purpose-built call, but it fails fast when
            // *no* instance exists at that instant, so its failure is not final
            // and we fall through to the deadline check.
            Err(e) if e.code() == ERROR_PIPE_BUSY.into() => {
                last_busy = true;
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    return Err(io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "the automation bridge is already serving another client",
                    ));
                }
                let ms = u32::try_from(remaining.as_millis()).unwrap_or(u32::MAX - 1);
                // SAFETY: same NUL-terminated name.
                if !unsafe { WaitNamedPipeW(PCWSTR(name.as_ptr()), ms.min(200)) }.as_bool() {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            // Not found: the server is between instances. Real absence and a
            // momentary gap look identical here, so retry until the deadline
            // and only then report it.
            Err(e) => {
                if std::time::Instant::now() >= deadline {
                    let kind = if last_busy {
                        io::ErrorKind::WouldBlock
                    } else {
                        io::ErrorKind::NotFound
                    };
                    return Err(io::Error::new(
                        kind,
                        format!("connecting to {address}: {e}"),
                    ));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_names_are_per_process_and_well_formed() {
        let n = pipe_name(4242);
        assert!(n.starts_with(r"\\.\pipe\"), "{n}");
        assert!(n.ends_with("-4242"), "{n}");
        assert!(n.len() < 256, "pipe names are capped at 256 characters");
    }

    #[test]
    fn the_current_user_sid_is_resolvable() {
        // The DACL is built from this; if it cannot be read, `bind` must fail
        // rather than fall back to the permissive default descriptor.
        let sid = current_user_sid_string().expect("current user SID");
        assert!(sid.starts_with("S-1-"), "{sid}");
    }

    #[test]
    fn the_descriptor_is_owner_only_and_protected() {
        let sec = OwnerOnlySecurity::current_user().expect("descriptor");
        assert!(!sec.descriptor.is_invalid());
        let attrs = sec.attributes();
        assert_eq!(
            attrs.nLength as usize,
            std::mem::size_of::<SECURITY_ATTRIBUTES>()
        );
        assert!(!attrs.lpSecurityDescriptor.is_null());
        assert!(
            !attrs.bInheritHandle.as_bool(),
            "handles must not be inheritable"
        );
    }

    /// The eager pre-creation in `accept` only works if the pipe allows a
    /// second instance while the first is in use. If this fails, every
    /// "there is always something listening" guarantee above is a fiction.
    #[test]
    fn a_second_instance_can_exist_while_the_first_is_in_use() {
        let pid = std::process::id().wrapping_add(31);
        let listener = PipeListener {
            name: wide(&pipe_name(pid)),
            security: OwnerOnlySecurity::current_user().expect("descriptor"),
            pending: None,
        };
        let first = listener.create_instance().expect("first instance");
        let second = listener
            .create_instance()
            .expect("a second instance must be creatable while the first is open");
        unsafe {
            let _ = CloseHandle(first);
            let _ = CloseHandle(second);
        }
    }

    /// A client must be able to connect right after the previous one has been
    /// served and released.
    ///
    /// Regression test for a real failure: `--list` probes the bridge by
    /// connecting and dropping, and the `--attach` a moment later was refused
    /// with ERROR_PIPE_BUSY. Two things caused it. `accept` creates the next
    /// instance eagerly so a client arriving the instant the previous one ends
    /// still finds somewhere to go — but at `nMaxInstances = 1` that creation
    /// always fails, leaving a gap with nothing listening. And a client that
    /// met that gap gave up at once instead of waiting for the server to come
    /// back around.
    ///
    /// Deliberately single-threaded: connecting before accepting is legal (the
    /// instance from `bind` is already listening, which is the documented
    /// ERROR_PIPE_CONNECTED path), and it keeps the test from ever leaving a
    /// thread parked in `accept` if an assertion fails.
    #[test]
    fn a_client_can_connect_after_the_previous_one_is_released() {
        let pid = std::process::id().wrapping_add(11);
        let mut bound = bind(pid).expect("bind");
        let address = bound.endpoint.address.clone();

        // A `--list`-style probe: connect, say nothing, drop.
        let probe = connect(&address).expect("probe connects");
        let peer1 = bound.listener.accept().expect("server accepts the probe");
        drop(probe);
        drop(peer1);

        // The attach that follows. This is the assertion that used to fail.
        let mut second =
            connect(&address).expect("a client must connect after the probe is released");
        let mut peer2 = bound
            .listener
            .accept()
            .expect("server accepts the second client");

        std::io::Write::write_all(&mut second, b"x").expect("client writes");
        std::io::Write::flush(&mut second).ok();
        let mut buf = [0u8; 1];
        std::io::Read::read_exact(&mut peer2, &mut buf).expect("server reads");
        assert_eq!(&buf, b"x", "the second client's bytes must arrive");
    }
}
