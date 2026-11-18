// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The election on Windows: one named pipe both decides and carries the
//! messages.
//!
//! The pipe is created with `FILE_FLAG_FIRST_PIPE_INSTANCE` and room for one
//! instance, so creating it fails when the name exists (with
//! `ERROR_PIPE_BUSY` for the one instance this allows, measured on Windows
//! 11, and with `ERROR_ACCESS_DENIED` for a pipe whose access list or flag
//! refuses it): whoever creates it is the primary, and keeps it for as long as it holds the handle. The system
//! closes that handle when the process ends, however it ends, so nothing is
//! left to clean up and a starter that finds the name gone retries the
//! election. A starter that finds it taken opens it as a client, and is a
//! secondary.
//!
//! Pipe names belong to the whole machine, so the name carries the user's
//! security identifier, and its access list grants that user alone. Another
//! user can still create a pipe of that name first, with an access list of
//! its own, so each side reads the user of the process at the other end from
//! its token and drops a peer of another user, as `SO_PEERCRED` does on
//! Unix. The client opens the pipe with `SECURITY_IDENTIFICATION`, so a
//! server can learn who it is but not act as it.
//!
//! There is one instance of the pipe, so the primary holds one connection at
//! a time: the next client finds it busy and waits with `WaitNamedPipeW`
//! until it disconnects. A client may connect before the primary listens
//! (a fresh instance takes a client before `ConnectNamedPipe`, which then
//! reports `ERROR_PIPE_CONNECTED`), so a second launch made while the first
//! is starting waits for it, as the Unix backlog does.
//!
//! A pipe has no receive timeout, so every handle is overlapped, and each
//! read and write waits on an event for what is left of its deadline,
//! cancelling the operation (and waiting for the cancellation to complete)
//! when the deadline passes. `DisconnectNamedPipe` discards what the client
//! has not read yet, so after writing its reply the primary waits, within
//! the same deadline, for the client to close its end.

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_BROKEN_PIPE, ERROR_FILE_NOT_FOUND, ERROR_IO_PENDING, ERROR_NO_DATA,
    ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, ERROR_PIPE_NOT_CONNECTED, GENERIC_READ, GENERIC_WRITE,
    HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT, WIN32_ERROR,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_SHARE_NONE,
    OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
    WriteFile,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    GetNamedPipeServerProcessId, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE,
    PIPE_WAIT, PeekNamedPipe, WaitNamedPipeW,
};
use windows::Win32::System::Threading::{
    CreateEventW, INFINITE, ResetEvent, SetEvent, WaitForMultipleObjects, WaitForSingleObject,
};
use windows::core::PCWSTR;

use super::protocol::{self, DecodeError, InstanceMessage, Reply};
use super::{ELECTION_PATIENCE, FORWARD_DEADLINE, PEER_DEADLINE, RETRY, check_key, fnv1a_64};
use crate::pipe_security::{OwnerOnlySecurity, ProcessUser, current_user_sid_string};

/// The pipe's buffer in each direction, a hint the system may round.
const PIPE_BUFFER: u32 = 64 * 1024;

/// The longest one `WaitNamedPipeW` lasts before the election is tried
/// again: the primary may leave meanwhile, which frees the name.
const BUSY_WAIT_SLICE: Duration = Duration::from_millis(100);

pub(super) enum Outcome {
    Primary(Primary),
    Secondary(Secondary),
}

pub(super) fn acquire(dir: &Path, key: &str) -> io::Result<Outcome> {
    acquire_within(dir, key, ELECTION_PATIENCE)
}

fn acquire_within(dir: &Path, key: &str, patience: Duration) -> io::Result<Outcome> {
    check_key(key)?;
    let name = pipe_name(dir, key, &current_user_sid_string()?);
    let wide_name = wide(&name);
    let security = OwnerOnlySecurity::current_user()?;
    let deadline = Instant::now() + patience;
    loop {
        match create_first_instance(&wide_name, &security) {
            Ok(handle) => return Ok(Outcome::Primary(Primary::new(Pipe::new(handle)?))),
            // The name exists: an instance holds it (`ERROR_PIPE_BUSY`, its
            // one instance taken), or a pipe another user made does.
            Err(e)
                if e.raw_os_error() == Some(ERROR_ACCESS_DENIED.0 as i32)
                    || e.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32) => {}
            Err(e) => return Err(e),
        }
        match open_client(&wide_name) {
            Ok(handle) => {
                let pipe = Pipe::new(handle)?;
                check_server(&pipe)?;
                return Ok(Outcome::Secondary(Secondary {
                    pipe,
                    deadline: FORWARD_DEADLINE,
                }));
            }
            // Serving another launch, holding one that waits for it to
            // listen, or between two connections.
            Err(e) if is(&e, ERROR_PIPE_BUSY) => {
                let left = deadline.saturating_duration_since(Instant::now());
                if !left.is_zero() {
                    // Not 0, which means the pipe's default wait. The result
                    // does not matter: the loop tries again either way.
                    let wait = millis(left.min(BUSY_WAIT_SLICE)).max(1);
                    // SAFETY: a NUL-terminated name that outlives the call.
                    let _ = unsafe { WaitNamedPipeW(PCWSTR(wide_name.as_ptr()), wait) };
                }
            }
            // The holder left between the two calls: the name is free, and
            // the next attempt takes it. The pause only keeps a state that
            // should not last from spinning.
            Err(e) if is(&e, ERROR_FILE_NOT_FOUND) => std::thread::sleep(Duration::from_millis(1)),
            // `ERROR_ACCESS_DENIED` here is a pipe of another user, closed to
            // this one: `PermissionDenied`.
            Err(e) => return Err(os_error(&e)),
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("an instance holds {name} but does not answer"),
            ));
        }
    }
}

/// `\\.\pipe\teksilo-si-<user SID>-<16 hex digits>`: the hash is [`fnv1a_64`]
/// of the directory's path as UTF-16 code units, each in two little-endian
/// bytes, a zero byte, and the key.
///
/// The path is hashed from its code units rather than from
/// `OsStr::as_encoded_bytes`, whose encoding `std` does not promise to keep.
/// The zero byte keeps the pair unambiguous: the key holds none, so the last
/// zero byte is the separator.
fn pipe_name(dir: &Path, key: &str, sid: &str) -> String {
    let mut bytes: Vec<u8> = dir
        .as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect();
    bytes.push(0);
    bytes.extend_from_slice(key.as_bytes());
    format!(r"\\.\pipe\teksilo-si-{sid}-{:016x}", fnv1a_64(&bytes))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Whether `e` is the Win32 error `code`.
fn is(e: &windows::core::Error, code: WIN32_ERROR) -> bool {
    e.code() == code.into()
}

/// `e` as an `io::Error` that keeps its Win32 code, so its kind is right
/// (`ERROR_ACCESS_DENIED` is `PermissionDenied`); the `From` conversion the
/// `windows` crate offers passes the HRESULT, whose kind is lost.
fn os_error(e: &windows::core::Error) -> io::Error {
    let hresult = e.code().0 as u32;
    if hresult & 0xffff_0000 == 0x8007_0000 {
        io::Error::from_raw_os_error((hresult & 0xffff) as i32)
    } else {
        io::Error::other(e.to_string())
    }
}

/// A wait in milliseconds, never `INFINITE`.
fn millis(d: Duration) -> u32 {
    u32::try_from(d.as_millis())
        .unwrap_or(u32::MAX)
        .min(INFINITE - 1)
}

fn timed_out() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "the connection's deadline expired")
}

/// Create the pipe's one instance, or fail because the name exists.
fn create_first_instance(name: &[u16], security: &OwnerOnlySecurity) -> io::Result<OwnedHandle> {
    let attributes = security.attributes();
    // SAFETY: `name` is NUL-terminated, and `attributes` points at a
    // descriptor `security` keeps alive, both for the whole call. The system
    // copies the descriptor into the pipe.
    let handle = unsafe {
        CreateNamedPipeW(
            PCWSTR(name.as_ptr()),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
            // One bitmask, set once: see the automation transport on
            // GHSA-7rrj-xr53-82p7, a later call that cleared the remote flag.
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            PIPE_BUFFER,
            PIPE_BUFFER,
            0,
            Some(&attributes),
        )
    };
    if handle.is_invalid() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a pipe handle just created, owned by nothing else.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle.0) })
}

/// Open the pipe as a client.
fn open_client(name: &[u16]) -> windows::core::Result<OwnedHandle> {
    // SAFETY: a NUL-terminated name that outlives the call. The handle
    // returned is valid (the wrapper turns an invalid one into an error).
    let handle = unsafe {
        CreateFileW(
            PCWSTR(name.as_ptr()),
            (GENERIC_READ | GENERIC_WRITE).0,
            FILE_SHARE_NONE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            None,
        )
    }?;
    // SAFETY: a handle just opened, owned by nothing else.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle.0) })
}

/// Drop a server that runs as another user: one may have made the pipe's
/// name before this user's instance did.
fn check_server(pipe: &Pipe) -> io::Result<()> {
    let mut pid = 0u32;
    // SAFETY: a connected pipe this function borrows; `pid` is a stack
    // out-parameter.
    unsafe { GetNamedPipeServerProcessId(pipe.raw(), &mut pid) }.map_err(|e| os_error(&e))?;
    check_same_user(pid)
}

/// Drop a client that runs as another user.
fn check_client(pipe: &Pipe) -> io::Result<()> {
    let mut pid = 0u32;
    // SAFETY: a connected pipe this function borrows; `pid` is a stack
    // out-parameter.
    unsafe { GetNamedPipeClientProcessId(pipe.raw(), &mut pid) }.map_err(|e| os_error(&e))?;
    check_same_user(pid)
}

/// Whether process `pid` runs as this user. A process whose user cannot be
/// read (it ended, or it runs elevated and this one does not) is refused.
fn check_same_user(pid: u32) -> io::Result<()> {
    let me = ProcessUser::current()?;
    let peer = ProcessUser::of_process(pid).map_err(|e| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("cannot tell which user the peer, process {pid}, runs as: {e}"),
        )
    })?;
    if !me.is(&peer) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("the peer, process {pid}, runs as another user"),
        ));
    }
    Ok(())
}

/// A manual-reset event.
#[derive(Debug)]
struct Event(OwnedHandle);

impl Event {
    fn new() -> io::Result<Self> {
        // SAFETY: an unnamed, manual-reset, unsignalled event with the
        // default security; the handle is owned below.
        let handle =
            unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.map_err(|e| os_error(&e))?;
        // SAFETY: an event handle just created, owned by nothing else.
        Ok(Self(unsafe { OwnedHandle::from_raw_handle(handle.0) }))
    }

    fn raw(&self) -> HANDLE {
        HANDLE(self.0.as_raw_handle())
    }

    fn set(&self) {
        // SAFETY: an event this value owns.
        let _ = unsafe { SetEvent(self.raw()) };
    }

    /// Wait for the event at most `timeout`; whether it is set.
    fn wait(&self, timeout: Duration) -> bool {
        // SAFETY: an event this value owns.
        unsafe { WaitForSingleObject(self.raw(), millis(timeout)) == WAIT_OBJECT_0 }
    }
}

/// What an overlapped operation waits for besides its own completion.
enum Until<'a> {
    Deadline(Instant),
    Stopped(&'a Event),
}

/// Why an overlapped operation did not complete.
enum OpError {
    /// It failed, at once or on completion.
    Os(windows::core::Error),
    /// Its deadline passed; it was cancelled.
    TimedOut,
    /// The stop event was set; it was cancelled.
    Stopped,
    /// Waiting for it failed; it was cancelled.
    Wait(io::Error),
}

impl OpError {
    fn into_io(self) -> io::Error {
        match self {
            OpError::Os(e) => os_error(&e),
            OpError::TimedOut => timed_out(),
            OpError::Stopped => io::Error::other("the instance stopped serving"),
            OpError::Wait(e) => e,
        }
    }
}

/// One end of the pipe, opened overlapped, and the event its operations
/// complete on. One operation at a time.
#[derive(Debug)]
struct Pipe {
    handle: OwnedHandle,
    event: Event,
}

impl Pipe {
    fn new(handle: OwnedHandle) -> io::Result<Self> {
        Ok(Self {
            handle,
            event: Event::new()?,
        })
    }

    fn raw(&self) -> HANDLE {
        HANDLE(self.handle.as_raw_handle())
    }

    /// Run one overlapped operation, which `start` begins with the
    /// `OVERLAPPED` it is handed, and return the bytes it moved.
    ///
    /// Never returns while the operation is pending: one that does not
    /// complete in time is cancelled, and the cancellation waited for, so the
    /// `OVERLAPPED` here and the buffer `start` borrows are free again.
    fn overlapped(
        &self,
        until: Until<'_>,
        start: impl FnOnce(*mut OVERLAPPED) -> windows::core::Result<()>,
    ) -> Result<u32, OpError> {
        // SAFETY: an event this pipe owns.
        unsafe { ResetEvent(self.event.raw()) }.map_err(OpError::Os)?;
        let mut ov = OVERLAPPED {
            hEvent: self.event.raw(),
            ..Default::default()
        };
        match start(&raw mut ov) {
            // Completed at once; its result is read below.
            Ok(()) => {}
            Err(e) if is(&e, ERROR_IO_PENDING) => {
                // SAFETY: waits on events this pipe and `until` own, alive for
                // the whole call.
                let waited = unsafe {
                    match until {
                        Until::Deadline(deadline) => WaitForSingleObject(
                            self.event.raw(),
                            millis(deadline.saturating_duration_since(Instant::now())),
                        ),
                        Until::Stopped(stop) => {
                            WaitForMultipleObjects(&[self.event.raw(), stop.raw()], false, INFINITE)
                        }
                    }
                };
                if waited != WAIT_OBJECT_0 {
                    let why = if waited == WAIT_TIMEOUT {
                        OpError::TimedOut
                    } else if waited.0 == WAIT_OBJECT_0.0 + 1 {
                        OpError::Stopped
                    } else {
                        OpError::Wait(io::Error::last_os_error())
                    };
                    self.cancel(&ov);
                    return Err(why);
                }
            }
            // Failed at once: nothing is pending.
            Err(e) => return Err(OpError::Os(e)),
        }
        let mut moved = 0u32;
        // SAFETY: the operation is complete, and `ov` is the one it ran with.
        unsafe { GetOverlappedResult(self.raw(), &ov, &mut moved, false) }.map_err(OpError::Os)?;
        Ok(moved)
    }

    /// Cancel the operation `ov` describes and wait until it is over, however
    /// it ends. `CancelIoEx` only asks; until the operation completes the
    /// system may still write into `ov` and its buffer.
    fn cancel(&self, ov: &OVERLAPPED) {
        let mut moved = 0u32;
        // SAFETY: `ov` describes an operation on this pipe's handle, and both
        // outlive the calls; the second waits for the operation to complete
        // (cancelled, or done before the cancellation landed), which it does
        // with `ov.hEvent`, this pipe's event.
        unsafe {
            let _ = CancelIoEx(self.raw(), Some(ov as *const OVERLAPPED));
            let _ = GetOverlappedResult(self.raw(), ov, &mut moved, true);
        }
    }
}

/// A pipe whose reads and writes share one deadline.
struct Timed<'a> {
    pipe: &'a Pipe,
    deadline: Instant,
}

impl Timed<'_> {
    fn check_left(&self) -> io::Result<()> {
        if Instant::now() >= self.deadline {
            return Err(timed_out());
        }
        Ok(())
    }

    /// Read until the peer closes its end or the deadline passes, dropping
    /// what it sends: `DisconnectNamedPipe` would discard a reply the peer
    /// has not read yet.
    fn until_closed(&mut self) {
        let mut sink = [0u8; 64];
        while let Ok(n) = self.read(&mut sink) {
            if n == 0 {
                return;
            }
        }
    }
}

impl Read for Timed<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.check_left()?;
        let len = buf.len().min(u32::MAX as usize);
        let buf = &mut buf[..len];
        let raw = self.pipe.raw();
        // SAFETY: `buf` outlives `overlapped`, which does not return while
        // the read is pending. The byte count is read from the `OVERLAPPED`,
        // as the documentation asks of an overlapped read.
        let result = self
            .pipe
            .overlapped(Until::Deadline(self.deadline), |ov| unsafe {
                ReadFile(raw, Some(buf), None, Some(ov))
            });
        match result {
            Ok(n) => Ok(n as usize),
            // The peer closed its end, or the server disconnected it.
            Err(OpError::Os(e))
                if is(&e, ERROR_BROKEN_PIPE) || is(&e, ERROR_PIPE_NOT_CONNECTED) =>
            {
                Ok(0)
            }
            Err(e) => Err(e.into_io()),
        }
    }
}

impl Write for Timed<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.check_left()?;
        let buf = &buf[..buf.len().min(u32::MAX as usize)];
        let raw = self.pipe.raw();
        // SAFETY: as for `read`.
        let result = self
            .pipe
            .overlapped(Until::Deadline(self.deadline), |ov| unsafe {
                WriteFile(raw, Some(buf), None, Some(ov))
            });
        result.map(|n| n as usize).map_err(OpError::into_io)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
pub(super) struct Primary {
    /// The pipe, until [`listen`](Self::listen) hands it to the serving
    /// thread.
    pipe: Option<Pipe>,
    /// The serving thread, once started.
    running: Option<Running>,
    peer_deadline: Duration,
}

/// What the instance keeps of its serving thread.
#[derive(Debug)]
struct Running {
    /// Set to stop it.
    stop: Arc<Event>,
    /// Set by it once it has closed the pipe.
    released: Arc<Event>,
}

impl Primary {
    fn new(pipe: Pipe) -> Self {
        Self {
            pipe: Some(pipe),
            running: None,
            peer_deadline: PEER_DEADLINE,
        }
    }

    pub(super) fn listen(
        &mut self,
        on_message: Box<dyn Fn(InstanceMessage) + Send>,
    ) -> io::Result<()> {
        if self.pipe.is_none() {
            return Err(io::Error::other("this instance is already listening"));
        }
        let stop = Arc::new(Event::new()?);
        let released = Arc::new(Event::new()?);
        // The thread is started before it is given the pipe, so a thread
        // that cannot start leaves this instance its pipe, and the name it
        // holds can still be served by a later call.
        let (hand_over, handed) = mpsc::sync_channel::<Serving>(1);
        std::thread::Builder::new()
            .name("teksilo-single-instance".into())
            .spawn(move || {
                if let Ok(serving) = handed.recv() {
                    serving.run();
                }
            })?;
        let serving = Serving {
            pipe: self.pipe.take().expect("checked above"),
            stop: Arc::clone(&stop),
            peer_deadline: self.peer_deadline,
            on_message,
            _released: Released(Arc::clone(&released)),
        };
        // The thread holds the receiving end until it has received.
        let _ = hand_over.send(serving);
        self.running = Some(Running { stop, released });
        Ok(())
    }
}

impl Drop for Primary {
    fn drop(&mut self) {
        // Before `listen` the pipe is closed with `self`, after these lines.
        if let Some(running) = self.running.take() {
            running.stop.set();
            // The name is the pipe, which the thread closes once the
            // connection it is serving, if any, is over.
            running.released.wait(self.peer_deadline);
        }
    }
}

/// Sets its event when dropped: after the pipe, the field declared before it
/// in [`Serving`], however the thread ends.
struct Released(Arc<Event>);

impl Drop for Released {
    fn drop(&mut self) {
        self.0.set();
    }
}

/// What the serving thread owns. Fields drop in order: the pipe first,
/// `_released` last.
struct Serving {
    pipe: Pipe,
    stop: Arc<Event>,
    peer_deadline: Duration,
    on_message: Box<dyn Fn(InstanceMessage) + Send>,
    /// Never read: dropped last, it says the pipe is closed.
    _released: Released,
}

/// How waiting for a client ended.
enum Connection {
    /// A client is connected.
    Made,
    /// A client connected and left before it was served.
    Gone,
    /// The instance is being dropped.
    Stopped,
    /// Something else, which may last.
    Failed,
}

impl Serving {
    fn run(self) {
        loop {
            if self.stop.wait(Duration::ZERO) {
                return;
            }
            match self.connect() {
                Connection::Made => {
                    let _ = self.answer();
                }
                Connection::Gone => {}
                Connection::Stopped => return,
                Connection::Failed => {
                    if self.stop.wait(RETRY) {
                        return;
                    }
                }
            }
            // SAFETY: the pipe this thread owns, with no operation pending on
            // it. On an instance with no client it fails, harmlessly.
            let _ = unsafe { DisconnectNamedPipe(self.pipe.raw()) };
        }
    }

    fn connect(&self) -> Connection {
        let raw = self.pipe.raw();
        // SAFETY: the pipe this thread owns, with the `OVERLAPPED`
        // `overlapped` keeps alive until the operation is over.
        let result = self
            .pipe
            .overlapped(Until::Stopped(&self.stop), |ov| unsafe {
                ConnectNamedPipe(raw, Some(ov))
            });
        match result {
            Ok(_) => Connection::Made,
            // A client connected before the call: before `listen`, or since
            // the last disconnection.
            Err(OpError::Os(e)) if is(&e, ERROR_PIPE_CONNECTED) => Connection::Made,
            Err(OpError::Os(e))
                if is(&e, ERROR_NO_DATA)
                    || is(&e, ERROR_BROKEN_PIPE)
                    || is(&e, ERROR_PIPE_NOT_CONNECTED) =>
            {
                Connection::Gone
            }
            Err(OpError::Stopped) => Connection::Stopped,
            Err(_) => Connection::Failed,
        }
    }

    /// Serve one connection: read its request within the deadline, hand it
    /// over, answer. A peer of another user, or one that sends anything
    /// malformed, is disconnected without an answer.
    fn answer(&self) -> io::Result<()> {
        check_client(&self.pipe)?;
        let mut io = Timed {
            pipe: &self.pipe,
            deadline: Instant::now() + self.peer_deadline,
        };
        let request = protocol::read_frame(&mut io, protocol::MAX_REQUEST)?;
        let reply = match protocol::decode_request(&request) {
            // A launch that gave up waiting has reported a failure, and its
            // request, still in the pipe, must not be acted on as well.
            Ok(_) if peer_hung_up(&self.pipe) => {
                return Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "the second launch stopped waiting",
                ));
            }
            Ok(message) => {
                (self.on_message)(message);
                Reply::Accepted
            }
            Err(DecodeError::Version(_)) => Reply::Rejected {
                version: protocol::VERSION,
            },
            Err(DecodeError::Malformed(why)) => {
                return Err(io::Error::new(io::ErrorKind::InvalidData, why));
            }
        };
        protocol::write_frame(&mut io, &protocol::encode_reply(reply))?;
        io.until_closed();
        Ok(())
    }
}

/// Whether the client has closed its end. Its request, already read, is all
/// it sent, so a peek that fails (`ERROR_BROKEN_PIPE`) means it is gone; one
/// that finds nothing means it is still waiting for the answer.
fn peer_hung_up(pipe: &Pipe) -> bool {
    let mut available = 0u32;
    // SAFETY: no buffer is passed; `available` is a stack out-parameter, on a
    // pipe this function borrows. PeekNamedPipe returns at once.
    unsafe { PeekNamedPipe(pipe.raw(), None, 0, None, Some(&mut available), None) }.is_err()
}

#[derive(Debug)]
pub(super) struct Secondary {
    pipe: Pipe,
    deadline: Duration,
}

impl Secondary {
    pub(super) fn forward(
        self,
        args: &[OsString],
        cwd: &Path,
        activation_token: Option<&str>,
    ) -> io::Result<()> {
        let request = protocol::encode_request(args, cwd, activation_token)?;
        let mut io = Timed {
            pipe: &self.pipe,
            deadline: Instant::now() + self.deadline,
        };
        protocol::write_frame(&mut io, &request)?;
        let reply = protocol::read_frame(&mut io, protocol::MAX_REPLY)?;
        match protocol::decode_reply(&reply)? {
            Reply::Accepted => Ok(()),
            Reply::Rejected { version } => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "the running instance speaks version {version} of the single-instance \
                     protocol, this one version {}",
                    protocol::VERSION
                ),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::MAX_KEY;
    use super::*;
    use std::fs::{File, OpenOptions};
    use std::path::PathBuf;

    /// A directory for one test. Pipe names belong to the whole machine and
    /// tests run in parallel, so each test, in each run, gets a name of its
    /// own. It is never created: only its path is hashed.
    fn dir(test: &str) -> PathBuf {
        PathBuf::from(format!(
            r"C:\teksilo-single-instance-test\{}\{test}",
            std::process::id()
        ))
    }

    fn name_of(dir: &Path) -> String {
        pipe_name(dir, "app", &current_user_sid_string().unwrap())
    }

    fn primary(outcome: Outcome) -> Primary {
        match outcome {
            Outcome::Primary(p) => p,
            Outcome::Secondary(_) => panic!("expected to be the primary"),
        }
    }

    fn secondary(outcome: Outcome) -> Secondary {
        match outcome {
            Outcome::Secondary(s) => s,
            Outcome::Primary(_) => panic!("expected to be a secondary"),
        }
    }

    /// A primary that sends what it receives down a channel.
    fn listening(
        dir: &Path,
        key: &str,
        peer_deadline: Duration,
    ) -> (Primary, mpsc::Receiver<InstanceMessage>) {
        let mut p = primary(acquire(dir, key).unwrap());
        p.peer_deadline = peer_deadline;
        let (tx, rx) = mpsc::channel();
        p.listen(Box::new(move |m| {
            let _ = tx.send(m);
        }))
        .unwrap();
        (p, rx)
    }

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    /// A client that speaks raw bytes, waiting for the pipe while it is busy.
    fn raw_peer(name: &str) -> File {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match OpenOptions::new().read(true).write(true).open(name) {
                Ok(file) => return file,
                Err(e)
                    if e.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32)
                        && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => panic!("opening {name}: {e}"),
            }
        }
    }

    /// The name is a stable hash of the directory and the key, under the
    /// user's SID, and fits a pipe name's 256 characters whatever their
    /// length.
    #[test]
    fn the_pipe_name_is_a_stable_hash_of_the_directory_and_the_key() {
        let mut bytes = vec![b'a', 0, 0];
        bytes.extend_from_slice(b"k");
        assert_eq!(
            pipe_name(Path::new("a"), "k", "S-1-5-21-1"),
            format!(r"\\.\pipe\teksilo-si-S-1-5-21-1-{:016x}", fnv1a_64(&bytes))
        );
        let sid = current_user_sid_string().unwrap();
        let one = pipe_name(Path::new(r"C:\a"), "app", &sid);
        assert_ne!(one, pipe_name(Path::new(r"C:\b"), "app", &sid));
        assert_ne!(one, pipe_name(Path::new(r"C:\a"), "other", &sid));
        let long = pipe_name(&PathBuf::from("d".repeat(1000)), &"k".repeat(MAX_KEY), &sid);
        assert!(long.len() < 256, "{long}");
    }

    /// What the election relies on: a second creation of the name fails with
    /// one of the two codes the election reads as "taken" (Windows 11 answers
    /// `ERROR_PIPE_BUSY`, the one instance being in use), and the name is
    /// free once the first is closed.
    #[test]
    fn the_first_instance_flag_refuses_a_second_one() {
        let name = wide(&name_of(&dir("first-instance")));
        let security = OwnerOnlySecurity::current_user().unwrap();
        let first = create_first_instance(&name, &security).unwrap();
        let err = create_first_instance(&name, &security).unwrap_err();
        assert!(
            err.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32)
                || err.raw_os_error() == Some(ERROR_ACCESS_DENIED.0 as i32),
            "{err:?}"
        );
        drop(first);
        let _again = create_first_instance(&name, &security).unwrap();
    }

    /// What the "gave up" check reads, once the request is read: a client
    /// still open is waiting, one that closed is gone. (Before the read the
    /// peek succeeds either way: what the client sent is still there.)
    #[test]
    fn a_client_that_closed_reads_as_hung_up() {
        let name = wide(&name_of(&dir("hung-up")));
        let security = OwnerOnlySecurity::current_user().unwrap();
        let server = Pipe::new(create_first_instance(&name, &security).unwrap()).unwrap();
        let client = Pipe::new(open_client(&name).unwrap()).unwrap();
        assert!(!peer_hung_up(&server), "open and silent");
        let mut io = Timed {
            pipe: &client,
            deadline: Instant::now() + Duration::from_secs(5),
        };
        io.write_all(b"request").unwrap();
        let mut io = Timed {
            pipe: &server,
            deadline: Instant::now() + Duration::from_secs(5),
        };
        let mut request = [0u8; 7];
        io.read_exact(&mut request).unwrap();
        assert_eq!(&request, b"request");
        assert!(!peer_hung_up(&server), "open, its request read");
        drop(client);
        assert!(peer_hung_up(&server), "closed, its request read");
    }

    #[test]
    fn the_second_election_in_one_process_is_a_secondary() {
        let dir = dir("second-election");
        let first = primary(acquire(&dir, "app").unwrap());
        let _second = secondary(acquire(&dir, "app").unwrap());
        // Another key is another instance.
        let _other = primary(acquire(&dir, "other").unwrap());
        drop(first);
    }

    #[test]
    fn the_server_and_the_client_check_each_other_as_this_user() {
        let dir = dir("check");
        let p = primary(acquire(&dir, "app").unwrap());
        let s = secondary(acquire(&dir, "app").unwrap());
        check_server(&s.pipe).unwrap();
        check_client(p.pipe.as_ref().unwrap()).unwrap();
    }

    #[test]
    fn a_message_round_trips_with_and_without_a_token() {
        use std::os::windows::ffi::OsStringExt;
        let dir = dir("round-trip");
        let (_p, rx) = listening(&dir, "app", PEER_DEADLINE);
        // An argument with an unpaired surrogate, which is not Unicode.
        let raw = OsString::from_wide(&[0x66, 0xd800, 0x2e, 0x74]);
        let sent = vec![OsString::from("app"), raw];
        for token in [None, Some("token-1")] {
            secondary(acquire(&dir, "app").unwrap())
                .forward(&sent, Path::new(r"C:\somewhere"), token)
                .unwrap();
            let got = rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(got.args, sent);
            assert_eq!(got.cwd, Path::new(r"C:\somewhere"));
            assert_eq!(got.activation_token.as_deref(), token);
        }
    }

    #[test]
    fn a_connection_made_before_listen_is_served_once_it_starts() {
        let dir = dir("before-listen");
        let mut p = primary(acquire(&dir, "app").unwrap());
        let s = secondary(acquire(&dir, "app").unwrap());
        let sent = std::thread::spawn(move || s.forward(&args(&["early"]), Path::new("/"), None));
        std::thread::sleep(Duration::from_millis(100));
        let (tx, rx) = mpsc::channel();
        p.listen(Box::new(move |m| {
            let _ = tx.send(m);
        }))
        .unwrap();
        sent.join().unwrap().unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap().args,
            args(&["early"])
        );
    }

    /// The one instance holds one connection: a second launch made while a
    /// first waits for the primary to listen waits for its turn, and gives up
    /// at its patience.
    #[test]
    fn a_second_launch_waits_while_the_one_connection_is_held() {
        let dir = dir("held");
        let _p = primary(acquire(&dir, "app").unwrap());
        let _first = secondary(acquire(&dir, "app").unwrap());
        let started = Instant::now();
        let err = acquire_within(&dir, "app", Duration::from_millis(300))
            .err()
            .expect("busy");
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_secondary_gives_up_on_a_primary_that_never_listens() {
        let dir = dir("never-listens");
        let _p = primary(acquire(&dir, "app").unwrap());
        let mut s = secondary(acquire(&dir, "app").unwrap());
        s.deadline = Duration::from_millis(200);
        let started = Instant::now();
        let err = s.forward(&args(&["x"]), Path::new("/"), None).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    /// A launch that timed out before the primary listened has reported a
    /// failure, so its request is not delivered.
    #[test]
    fn a_launch_that_gave_up_is_not_delivered() {
        let dir = dir("gave-up");
        let mut p = primary(acquire(&dir, "app").unwrap());
        let mut s = secondary(acquire(&dir, "app").unwrap());
        s.deadline = Duration::from_millis(200);
        let err = s
            .forward(&args(&["gave-up"]), Path::new("/"), None)
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);

        let (tx, rx) = mpsc::channel();
        p.listen(Box::new(move |m| {
            let _ = tx.send(m);
        }))
        .unwrap();
        secondary(acquire(&dir, "app").unwrap())
            .forward(&args(&["waited"]), Path::new("/"), None)
            .unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap().args,
            args(&["waited"])
        );
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn listening_twice_is_refused() {
        let (mut p, _rx) = listening(&dir("twice"), "app", PEER_DEADLINE);
        assert!(p.listen(Box::new(|_| {})).is_err());
    }

    #[test]
    fn keys_outside_the_alphabet_or_too_long_are_refused() {
        let dir = dir("keys");
        for key in ["", "a/b", "a b", "é", "../x", &"k".repeat(MAX_KEY + 1)] {
            let err = acquire(&dir, key).err().expect("refused");
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{key:?}");
        }
        let _p = primary(acquire(&dir, "eu.ferntech.App-2_x").unwrap());
    }

    #[test]
    fn dropping_the_primary_frees_the_name() {
        let dir = dir("drop");
        let (p, _rx) = listening(&dir, "app", PEER_DEADLINE);
        drop(p);
        let unheard = primary(acquire(&dir, "app").unwrap());
        drop(unheard);
        let _p = primary(acquire(&dir, "app").unwrap());
    }

    /// Starters racing for one name: exactly one is the primary, and every
    /// other one is served by it.
    #[test]
    fn racing_starters_elect_one_primary() {
        const STARTERS: usize = 8;
        let dir = dir("race");
        let (tx, rx) = mpsc::channel();
        let barrier = Arc::new(std::sync::Barrier::new(STARTERS));
        let starters: Vec<_> = (0..STARTERS)
            .map(|i| {
                let (dir, tx, barrier) = (dir.clone(), tx.clone(), Arc::clone(&barrier));
                std::thread::spawn(move || {
                    barrier.wait();
                    match acquire(&dir, "app").unwrap() {
                        Outcome::Primary(mut p) => {
                            p.listen(Box::new(move |m| {
                                let _ = tx.send(m);
                            }))
                            .unwrap();
                            Some(p)
                        }
                        Outcome::Secondary(s) => {
                            s.forward(&args(&[&i.to_string()]), Path::new("/"), None)
                                .unwrap();
                            None
                        }
                    }
                })
            })
            .collect();
        drop(tx);
        let primaries: Vec<Primary> = starters
            .into_iter()
            .filter_map(|t| t.join().unwrap())
            .collect();
        assert_eq!(primaries.len(), 1);
        let mut got: Vec<_> = (1..STARTERS)
            .map(|_| rx.recv_timeout(Duration::from_secs(5)).unwrap().args)
            .collect();
        got.sort();
        got.dedup();
        assert_eq!(got.len(), STARTERS - 1, "every secondary, once");
    }

    /// Bad peers are each dropped within the deadline, and a good one is
    /// served after them.
    #[test]
    fn bad_peers_are_dropped_and_the_listener_keeps_serving() {
        let deadline = Duration::from_millis(300);
        let dir = dir("bad-peers");
        let (_p, rx) = listening(&dir, "app", deadline);
        let name = name_of(&dir);

        // Disconnected without an answer, within `deadline` and well under
        // the 5 seconds the test allows. The read runs on a thread of its
        // own, so a server that never disconnects fails the test rather than
        // hanging it.
        let closed_in_time = |mut peer: File| {
            let started = Instant::now();
            let (done, finished) = mpsc::channel();
            std::thread::spawn(move || {
                let mut buf = [0u8; 8];
                let _ = done.send(peer.read(&mut buf).unwrap_or(0));
            });
            let n = finished
                .recv_timeout(Duration::from_secs(5))
                .expect("disconnected");
            assert_eq!(n, 0, "no answer");
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "{:?}",
                started.elapsed()
            );
        };

        // Says nothing.
        closed_in_time(raw_peer(&name));

        // An oversized length.
        let mut peer = raw_peer(&name);
        peer.write_all(&u32::MAX.to_le_bytes()).unwrap();
        closed_in_time(peer);

        // Garbage of the right size.
        let mut peer = raw_peer(&name);
        protocol::write_frame(&mut peer, &[protocol::VERSION, 0xff, 0xff]).unwrap();
        closed_in_time(peer);

        // One byte every 100 ms: each write is quick, the whole is not. The
        // writes start failing once the primary has disconnected.
        let mut peer = raw_peer(&name);
        let mut frame = Vec::new();
        protocol::write_frame(&mut frame, &[0u8; 1000]).unwrap();
        let started = Instant::now();
        let mut cut = false;
        for byte in frame {
            if peer.write_all(&[byte]).is_err() {
                cut = true;
                break;
            }
            if started.elapsed() > Duration::from_secs(3) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(cut, "still connected after {:?}", started.elapsed());
        assert!(started.elapsed() < Duration::from_secs(2));
        drop(peer);

        // Another version is answered as such.
        let mut peer = raw_peer(&name);
        protocol::write_frame(&mut peer, &[protocol::VERSION + 1]).unwrap();
        let (done, finished) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = done.send(protocol::read_frame(&mut peer, protocol::MAX_REPLY));
        });
        let reply = finished
            .recv_timeout(Duration::from_secs(5))
            .expect("an answer")
            .unwrap();
        assert_eq!(
            protocol::decode_reply(&reply).unwrap(),
            Reply::Rejected {
                version: protocol::VERSION
            }
        );

        assert!(rx.try_recv().is_err(), "nothing bad was handed over");
        secondary(acquire(&dir, "app").unwrap())
            .forward(&args(&["good"]), Path::new("/"), None)
            .unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap().args,
            args(&["good"])
        );
    }
}
