// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The live-bridge wire protocol, and how a client finds a bridge to talk to.
//!
//! Everything here is pure `std` + `serde`: framing, the token handshake, and
//! the on-disk endpoint descriptor. Nothing in this module opens a socket or a
//! pipe — that is
//! [`teksilo_platform::automation_transport`](../../teksilo_platform/automation_transport/index.html)'s
//! job. The split is deliberate: the protocol is identical on every platform
//! and is exhaustively testable over an in-memory buffer, while only
//! `bind`/`accept`/`connect` and their access control differ per OS.
//!
//! ## Framing
//!
//! After the connection opens, the client sends its token as one `\n`-terminated
//! line. Every message thereafter — in both directions — is a **4-byte
//! little-endian length prefix followed by that many bytes of UTF-8 JSON**.
//! One request is in flight at a time, so replies need no correlation id.
//!
//! ```text
//! client → server   "<token>\n"
//! client → server   [len:u32-le][{"op":{...},"settle":{...}}]
//! server → client   [len:u32-le][{"status":"ok","data":{...}}]
//! ```
//!
//! ## Discovery
//!
//! A running app writes an [`EndpointFile`] naming its transport, address and
//! token. A client reads it instead of scraping stderr — which is what makes
//! one CLI work across a Unix socket path and a Windows pipe name, and removes
//! the race the old announcement had (the client acted on a printed path the
//! instant it appeared, and never retried).

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Cap on one inbound **request** frame.
///
/// Requests are small JSON ops — no images travel inbound — so this exists to
/// bound the `vec![0u8; len]` allocation against a client that sends a bogus
/// 4-byte length (up to ~4 GiB otherwise).
pub const MAX_REQUEST_FRAME: usize = 16 * 1024 * 1024;

/// Cap on one **reply** frame. Larger than a request because a reply can carry
/// a base64 screenshot: a 5K display captures ~59 MB of raw pixels, and PNG +
/// base64 must fit under this on both ends.
pub const MAX_REPLY_FRAME: usize = 256 * 1024 * 1024;

/// Cap on the token handshake line, so a peer that connects and streams bytes
/// without ever sending a newline cannot exhaust memory.
pub const MAX_TOKEN_LINE: u64 = 512;

/// Schema version of the [`EndpointFile`]. Bumped only on a breaking change;
/// a reader rejects anything it does not recognise rather than guessing.
pub const ENDPOINT_FILE_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Framing
// ---------------------------------------------------------------------------

/// Write one length-prefixed frame and flush it.
///
/// Errors with [`io::ErrorKind::InvalidInput`] if `bytes` exceeds `max` — a
/// sender that would produce an unreadable frame should fail locally, where
/// the error is actionable, rather than desync the peer.
pub fn write_frame(w: &mut impl Write, bytes: &[u8], max: usize) -> io::Result<()> {
    if bytes.len() > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "frame of {} bytes exceeds the {max}-byte limit",
                bytes.len()
            ),
        ));
    }
    w.write_all(&(bytes.len() as u32).to_le_bytes())?;
    w.write_all(bytes)?;
    w.flush()
}

/// Read one length-prefixed frame.
///
/// A clean EOF on the length prefix (peer disconnected between frames) is
/// reported as [`io::ErrorKind::UnexpectedEof`], which callers treat as "the
/// conversation ended", not as an error to report.
pub fn read_frame(r: &mut impl Read, max: usize) -> io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let frame_len = u32::from_le_bytes(len) as usize;
    if frame_len > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame of {frame_len} bytes exceeds the {max}-byte limit"),
        ));
    }
    let mut buf = vec![0u8; frame_len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

/// Send the token handshake line.
pub fn write_token(w: &mut impl Write, token: &str) -> io::Result<()> {
    w.write_all(token.as_bytes())?;
    w.write_all(b"\n")?;
    w.flush()
}

/// Read the token handshake line, bounded by [`MAX_TOKEN_LINE`].
///
/// Reads a byte at a time rather than through a [`std::io::BufRead`]: a buffered reader
/// would happily pull the first *frame* into its buffer along with the token
/// line, so the caller would then need a second handle on the connection to
/// read the rest — which is what forced the old code to `try_clone` the socket,
/// an operation with no clean equivalent on a Windows pipe. 512 one-byte reads,
/// once per connection, buys a transport-agnostic handshake.
///
/// Returns the trimmed token. Compare it with [`token_matches`] rather than
/// `==`, so the comparison does not leak length or prefix through timing.
pub fn read_token(r: &mut impl Read) -> io::Result<String> {
    read_token_by(r, None)
}

/// [`read_token`], bounded by a deadline for the handshake **as a whole**.
///
/// A transport-level read timeout is per-`read`, and this reads one byte at a
/// time — so on its own a 10 s read deadline lets a peer drip one byte every
/// 9 s and hold the single connection slot for `MAX_TOKEN_LINE` × 10 s, which
/// is the exact denial the deadline exists to prevent. The caller passes the
/// bound it actually means; it is checked between bytes.
pub fn read_token_by(
    r: &mut impl Read,
    deadline: Option<std::time::Instant>,
) -> io::Result<String> {
    let mut line = Vec::with_capacity(64);
    let mut byte = [0u8; 1];
    while (line.len() as u64) < MAX_TOKEN_LINE {
        if deadline.is_some_and(|d| std::time::Instant::now() >= d) {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the token handshake did not complete within its deadline",
            ));
        }
        match r.read(&mut byte) {
            Ok(0) => break, // EOF before the newline
            Ok(_) if byte[0] == b'\n' => break,
            Ok(_) => line.push(byte[0]),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(String::from_utf8_lossy(&line).trim().to_string())
}

/// Constant-time-ish token comparison.
///
/// The tokens are per-process UUIDs behind an OS access-control boundary, so
/// this is belt-and-braces rather than load-bearing — but a plain `==` returns
/// on the first differing byte, and there is no reason to hand that signal out.
pub fn token_matches(expected: &str, got: &str) -> bool {
    let (a, b) = (expected.as_bytes(), got.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

// ---------------------------------------------------------------------------
// Endpoint
// ---------------------------------------------------------------------------

/// Which local IPC mechanism a bridge is listening on.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    /// A Unix-domain socket at a filesystem path (Linux, macOS, BSD).
    Unix,
    /// A Windows named pipe under `\\.\pipe\`.
    NamedPipe,
}

impl Transport {
    /// The transport this platform uses.
    pub const fn native() -> Self {
        #[cfg(windows)]
        {
            Transport::NamedPipe
        }
        #[cfg(not(windows))]
        {
            Transport::Unix
        }
    }
}

/// A bridge's address: what kind of thing to open, and what to call it.
///
/// `address` is a filesystem path for [`Transport::Unix`] and a pipe name
/// (`\\.\pipe\…`) for [`Transport::NamedPipe`]. It is deliberately a `String`
/// rather than a `PathBuf`: a pipe name is not a path, and pretending
/// otherwise is what made the old CLI Unix-shaped.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// The mechanism.
    pub transport: Transport,
    /// The socket path or pipe name.
    pub address: String,
}

impl Endpoint {
    /// A Unix-socket endpoint at `path`.
    pub fn unix(path: impl Into<String>) -> Self {
        Self {
            transport: Transport::Unix,
            address: path.into(),
        }
    }

    /// A named-pipe endpoint called `name` (the full `\\.\pipe\…` form).
    pub fn named_pipe(name: impl Into<String>) -> Self {
        Self {
            transport: Transport::NamedPipe,
            address: name.into(),
        }
    }

    /// Guess the transport from an address, for the `--connect <addr>` escape
    /// hatch where the user typed one thing and meant the obvious one.
    pub fn from_address(address: &str) -> Self {
        if address.starts_with(r"\\") || address.starts_with("//") {
            Self::named_pipe(address)
        } else {
            Self::unix(address)
        }
    }
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.address)
    }
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// The descriptor a running app publishes so a client can attach to it.
///
/// Written to `<runtime dir>/teksilo-automation/<pid>.json`, owner-only. It
/// carries the token, so its permissions are part of the security model — see
/// [`EndpointFile::write`].
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct EndpointFile {
    /// [`ENDPOINT_FILE_VERSION`].
    pub version: u32,
    /// The publishing process.
    pub pid: u32,
    /// Where to connect.
    #[serde(flatten)]
    pub endpoint: Endpoint,
    /// The handshake token.
    pub token: String,
    /// The app's executable name, so a human choosing between two live apps
    /// can tell which is which.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// Wall-clock start time, used to pick the newest when several are live.
    #[serde(default)]
    pub started_unix_ms: u128,
}

impl EndpointFile {
    /// Build a descriptor for this process.
    pub fn new(endpoint: Endpoint, token: impl Into<String>) -> Self {
        Self {
            version: ENDPOINT_FILE_VERSION,
            pid: std::process::id(),
            endpoint,
            token: token.into(),
            app: std::env::current_exe()
                .ok()
                .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned())),
            started_unix_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
        }
    }

    /// The per-user directory holding endpoint descriptors.
    ///
    /// Each platform's answer is the directory that platform actually provides
    /// for per-user runtime state:
    ///
    /// - **Linux** — `$XDG_RUNTIME_DIR`, which is per-user `0700` by spec.
    /// - **macOS** — `std::env::temp_dir()`, i.e. `$TMPDIR`, which on Darwin is
    ///   the per-user per-boot `/var/folders/…/T/`. `$XDG_RUNTIME_DIR` is never
    ///   set by macOS, so the old fallback landed in the shared `/tmp`.
    /// - **Windows** — `%LOCALAPPDATA%`, per-user by ACL inheritance.
    pub fn dir() -> PathBuf {
        let base = runtime_dir();
        base.join("teksilo-automation")
    }

    /// This process's descriptor path.
    pub fn path_for_pid(pid: u32) -> PathBuf {
        Self::dir().join(format!("{pid}.json"))
    }

    /// Write the descriptor, owner-readable only.
    ///
    /// On Unix the containing directory is created `0700` and the file `0600`.
    /// On Windows both inherit `%LOCALAPPDATA%`'s per-user ACL. The file holds
    /// the token, so this is the difference between "another local user needs
    /// the token" and "another local user can read it".
    pub fn write(&self) -> io::Result<PathBuf> {
        let dir = Self::dir();
        create_private_dir(&dir)?;
        let path = Self::path_for_pid(self.pid);
        let json = serde_json::to_vec_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        // Write-then-rename so a reader never sees a half-written descriptor.
        let tmp = path.with_extension("json.tmp");
        // `create_new` + the mode in the *same* `open`, never create-then-chmod:
        // this file carries the token, and a `write`-then-`set_permissions`
        // leaves it at the umask default (0644 as a rule) for the window in
        // between — the same bind→chmod TOCTOU the socket ordering exists to
        // close. `create_new` also refuses to follow a symlink planted at the
        // temp path, so a writable parent cannot redirect the token elsewhere.
        let _ = std::fs::remove_file(&tmp);
        let mut file = private_file_options().create_new(true).open(&tmp)?;
        // The empty 0600 file we just created is proof of our own uid, so the
        // directory's owner can be checked with no `libc` and — crucially —
        // *before* the token reaches the disk. See `create_private_dir` for why
        // an existing directory is not taken on trust.
        if let Err(e) = check_dir_is_ours(&dir, &file) {
            drop(file);
            let _ = std::fs::remove_file(&tmp);
            return Err(e);
        }
        std::io::Write::write_all(&mut file, &json)?;
        drop(file);
        std::fs::rename(&tmp, &path)?;
        Ok(path)
    }

    /// Read one descriptor.
    pub fn read(path: &Path) -> io::Result<Self> {
        let bytes = std::fs::read(path)?;
        let parsed: Self = serde_json::from_slice(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if parsed.version != ENDPOINT_FILE_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "endpoint file version {} is not supported (this build speaks {ENDPOINT_FILE_VERSION})",
                    parsed.version
                ),
            ));
        }
        Ok(parsed)
    }

    /// Every readable descriptor in [`dir`](Self::dir), newest first.
    ///
    /// Unreadable or stale-format entries are skipped rather than failing the
    /// listing: one crashed app must not hide the others.
    pub fn list() -> Vec<Self> {
        let Ok(entries) = std::fs::read_dir(Self::dir()) else {
            return Vec::new();
        };
        let mut found: Vec<Self> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| Self::read(&e.path()).ok())
            .collect();
        found.sort_by_key(|f| std::cmp::Reverse(f.started_unix_ms));
        found
    }

    /// Remove this descriptor. Called when the bridge thread exits.
    pub fn remove(pid: u32) {
        let _ = std::fs::remove_file(Self::path_for_pid(pid));
    }
}

/// The per-user runtime directory for this platform. See [`EndpointFile::dir`].
fn runtime_dir() -> PathBuf {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(|local| PathBuf::from(local).join("Teksilo"))
            .unwrap_or_else(std::env::temp_dir)
    }
    #[cfg(not(windows))]
    {
        if let Some(xdg) = std::env::var_os("XDG_RUNTIME_DIR") {
            return PathBuf::from(xdg);
        }
        // macOS and any Unix without XDG: `temp_dir` honours `$TMPDIR`, which
        // Darwin's `launchd` sets to the per-user `/var/folders/…/T/`; std
        // resolves it via `confstr(_CS_DARWIN_USER_TEMP_DIR)` when unset.
        std::env::temp_dir()
    }
}

/// Create `dir` (and parents) private to the current user.
///
/// An **already existing** directory is not taken on trust. `$XDG_RUNTIME_DIR`
/// is per-user by spec and Darwin's `$TMPDIR` is per-user per-boot, but the
/// documented fallback for a Unix with neither is the *shared* `/tmp`, where
/// another local user can create `teksilo-automation/` first. Accepting
/// `AlreadyExists` blindly would then publish a token-bearing descriptor into a
/// directory somebody else controls — and let [`EndpointFile::list`] pick up
/// descriptors they planted. So an existing entry must be a real directory (not
/// a symlink) and is tightened to `0700` if it is not already; the *ownership*
/// half of the check needs a file we know we created and lives in
/// [`EndpointFile::write`], which runs it before the token reaches the disk.
pub fn create_private_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        if let Some(parent) = dir.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match std::fs::DirBuilder::new().mode(0o700).create(dir) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
        // `symlink_metadata`, so a symlink pointing at a directory we do own is
        // still rejected — the target is not what we would be protecting.
        let meta = std::fs::symlink_metadata(dir)?;
        if !meta.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} exists but is not a directory", dir.display()),
            ));
        }
        if meta.permissions().mode() & 0o077 != 0 {
            // Ours or not, it is reachable by others. An earlier
            // `create_dir_all` (which honours the umask, so 0755 as a rule) is
            // the common cause, so tighten in place rather than refuse — and
            // fail loudly if we are not the one who may.
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)
    }
}

/// Refuse to publish into a directory somebody else owns.
///
/// `owned_probe` must be a file this process has just created, so its uid *is*
/// our effective uid — which is how this manages an ownership check with no
/// `libc` and no extra dependency, keeping the module pure `std` + `serde`.
#[allow(unused_variables)]
fn check_dir_is_ours(dir: &Path, owned_probe: &std::fs::File) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let ours = owned_probe.metadata()?.uid();
        let theirs = std::fs::symlink_metadata(dir)?.uid();
        if ours != theirs {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "{} is owned by uid {theirs}, not by this user ({ours}) — refusing to publish \
                     the automation token into it",
                    dir.display()
                ),
            ));
        }
    }
    Ok(())
}

/// `OpenOptions` that create a file only the current user can read.
fn private_file_options() -> std::fs::OpenOptions {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"{\"op\":\"settle\"}", MAX_REQUEST_FRAME).unwrap();
        let mut cursor = io::Cursor::new(buf);
        let got = read_frame(&mut cursor, MAX_REQUEST_FRAME).unwrap();
        assert_eq!(got, b"{\"op\":\"settle\"}");
    }

    #[test]
    fn empty_frame_round_trips() {
        // A zero-length frame is legal and must not be mistaken for EOF.
        let mut buf = Vec::new();
        write_frame(&mut buf, b"", MAX_REQUEST_FRAME).unwrap();
        assert_eq!(buf, vec![0, 0, 0, 0]);
        let mut cursor = io::Cursor::new(buf);
        assert!(
            read_frame(&mut cursor, MAX_REQUEST_FRAME)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn several_frames_stream_back_in_order() {
        let mut buf = Vec::new();
        for n in 0..5u8 {
            write_frame(&mut buf, &[n; 3], MAX_REQUEST_FRAME).unwrap();
        }
        let mut cursor = io::Cursor::new(buf);
        for n in 0..5u8 {
            assert_eq!(read_frame(&mut cursor, MAX_REQUEST_FRAME).unwrap(), [n; 3]);
        }
        // Then a clean EOF, which callers read as "peer disconnected".
        let end = read_frame(&mut cursor, MAX_REQUEST_FRAME).unwrap_err();
        assert_eq!(end.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn oversize_frame_is_refused_on_write_and_read() {
        let mut buf = Vec::new();
        let err = write_frame(&mut buf, &[0u8; 64], 16).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(buf.is_empty(), "nothing is emitted for a refused frame");

        // A peer claiming a huge length must be refused before allocating.
        let mut hostile = Vec::new();
        hostile.extend_from_slice(&u32::MAX.to_le_bytes());
        let mut cursor = io::Cursor::new(hostile);
        let err = read_frame(&mut cursor, MAX_REQUEST_FRAME).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn truncated_frame_is_an_error_not_a_short_read() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"0123456789", MAX_REQUEST_FRAME).unwrap();
        buf.truncate(buf.len() - 4); // cut the payload short
        let mut cursor = io::Cursor::new(buf);
        let err = read_frame(&mut cursor, MAX_REQUEST_FRAME).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn token_line_round_trips_and_is_bounded() {
        let mut buf = Vec::new();
        write_token(&mut buf, "abc-123").unwrap();
        assert_eq!(buf, b"abc-123\n");
        let mut cursor = io::Cursor::new(buf);
        assert_eq!(read_token(&mut cursor).unwrap(), "abc-123");

        // A peer that never sends a newline is cut off at the cap rather than
        // being allowed to grow the buffer without bound.
        let flood = vec![b'x'; 4096];
        let mut cursor = io::Cursor::new(flood);
        let got = read_token(&mut cursor).unwrap();
        assert_eq!(got.len(), MAX_TOKEN_LINE as usize);
    }

    #[test]
    fn handshake_leaves_the_first_frame_intact() {
        // The reason `read_token` is byte-wise: the very next bytes on the wire
        // are a frame, and the handshake must not swallow any of them.
        let mut buf = Vec::new();
        write_token(&mut buf, "tok").unwrap();
        write_frame(&mut buf, b"first", MAX_REQUEST_FRAME).unwrap();
        write_frame(&mut buf, b"second", MAX_REQUEST_FRAME).unwrap();

        let mut cursor = io::Cursor::new(buf);
        assert_eq!(read_token(&mut cursor).unwrap(), "tok");
        assert_eq!(
            read_frame(&mut cursor, MAX_REQUEST_FRAME).unwrap(),
            b"first"
        );
        assert_eq!(
            read_frame(&mut cursor, MAX_REQUEST_FRAME).unwrap(),
            b"second"
        );
    }

    #[test]
    fn token_comparison_rejects_wrong_and_short() {
        assert!(token_matches("secret", "secret"));
        assert!(!token_matches("secret", "secrer"));
        assert!(!token_matches("secret", "secre"));
        assert!(!token_matches("secret", ""));
    }

    #[test]
    fn endpoint_address_shape_is_guessed_per_transport() {
        assert_eq!(
            Endpoint::from_address(r"\\.\pipe\teksilo-automation-9").transport,
            Transport::NamedPipe
        );
        assert_eq!(
            Endpoint::from_address("/run/user/1000/tka-9/s").transport,
            Transport::Unix
        );
    }

    #[test]
    fn endpoint_file_round_trips_through_json() {
        let ep = EndpointFile {
            version: ENDPOINT_FILE_VERSION,
            pid: 4242,
            endpoint: Endpoint::named_pipe(r"\\.\pipe\teksilo-automation-4242"),
            token: "tok".into(),
            app: Some("widget-catalog".into()),
            started_unix_ms: 17,
        };
        let json = serde_json::to_string(&ep).unwrap();
        // `transport` / `address` are flattened, so the file reads naturally.
        assert!(json.contains("\"transport\":\"named_pipe\""), "{json}");
        assert!(json.contains("\"address\":"), "{json}");
        assert_eq!(serde_json::from_str::<EndpointFile>(&json).unwrap(), ep);
    }

    #[test]
    fn endpoint_file_rejects_a_future_version() {
        let dir = std::env::temp_dir().join(format!("tka-wire-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("future.json");
        std::fs::write(
            &path,
            br#"{"version":9999,"pid":1,"transport":"unix","address":"/x","token":"t"}"#,
        )
        .unwrap();
        let err = EndpointFile::read(&path).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn runtime_dir_is_absolute() {
        // Every platform branch must yield something we can create under; a
        // relative path would scatter descriptors through the CWD.
        assert!(
            EndpointFile::dir().is_absolute(),
            "{:?}",
            EndpointFile::dir()
        );
    }

    /// The descriptor carries the token, so it must never exist — not even for
    /// an instant — at anything but `0600`.
    ///
    /// A `write`-then-`chmod` passes an after-the-fact permissions assertion
    /// while still leaving the token at the umask default in between, so this
    /// checks the *temporary* file's mode as well: that is the one the window
    /// belongs to, and it is created and left behind here on purpose.
    #[cfg(unix)]
    #[test]
    fn the_descriptor_is_never_briefly_world_readable() {
        use std::os::unix::fs::PermissionsExt;

        let file = EndpointFile::new(Endpoint::unix("/tmp/does-not-matter"), "tok");
        let Ok(path) = file.write() else {
            return; // a sandbox that cannot write its runtime dir is not a failure here
        };
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "the published descriptor must be owner-only");

        // The staging file is what a create-then-chmod would leak through.
        let tmp = path.with_extension("json.tmp");
        let staged = private_file_options().create_new(true).open(&tmp);
        if let Ok(f) = staged {
            let staged_mode = f.metadata().unwrap().permissions().mode() & 0o777;
            let _ = std::fs::remove_file(&tmp);
            assert_eq!(
                staged_mode, 0o600,
                "the staging file must be owner-only from the moment it exists"
            );
        }
        EndpointFile::remove(file.pid);
    }
}
