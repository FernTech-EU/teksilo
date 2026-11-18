// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What a second launch says to the running instance, and its answer.
//!
//! Pure: bytes in, values out, no I/O beyond the `Read` / `Write` the two
//! frame functions are handed, so every bound is tested over a buffer.
//!
//! One connection carries one exchange. The secondary sends one request
//! frame, the primary answers with one reply frame, and both close.
//!
//! ```text
//! frame    = len:u32-le, payload[len]
//! request  = version:u8,
//!            count:u32-le, { len:u32-le, arg[len] } * count,
//!            len:u32-le, cwd[len],
//!            0:u8 | 1:u8, len:u32-le, token[len]
//! reply    = 0:u8                 accepted
//!          | 1:u8, version:u8     rejected: the primary speaks `version`
//! ```
//!
//! The frame is the automation bridge's (`teksilo_automation::wire`): a
//! 4-byte little-endian length, then the payload. It is written again here
//! rather than shared. Using it from there would link the automation toolkit
//! into every single-instance application, release builds included, and it
//! cannot move here, since this crate already depends on `teksilo-automation`
//! (for the bridge's endpoint types) and the dependency cannot go both ways.
//!
//! Arguments and the working directory travel as the operating system gave
//! them: on Unix as their bytes, so a file name that is not UTF-8 reaches the
//! primary unchanged; on Windows as their UTF-16 code units, each in two
//! little-endian bytes, so one with an unpaired surrogate does too. Both ends
//! run on one machine, so the two encodings never meet.

use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// The protocol this build speaks. A request with another version is
/// answered [`Reply::Rejected`] with this one, so the secondary can say
/// which side is out of date.
pub(super) const VERSION: u8 = 1;

/// The most arguments one request may carry.
pub(super) const MAX_ARGS: usize = 65536;

/// The most bytes one request may hold, everything included: Linux's
/// `ARG_MAX` with the default 8 MiB stack, so an ordinary large command line
/// (a few thousand files from a file manager) fits. A Windows command line
/// is at most 32767 UTF-16 code units, 64 KiB, far below it.
pub(super) const MAX_REQUEST: usize = 2 * 1024 * 1024;

/// The longest activation token, in bytes. Wayland and X11 tokens are a few
/// dozen.
pub(super) const MAX_TOKEN: usize = 1024;

/// The most bytes one reply may hold.
pub(super) const MAX_REPLY: usize = 2;

/// A second launch's request, as the running instance receives it.
///
/// Every field is untrusted input: any process of the same user can connect
/// and send anything, so validate the arguments as you would a command line
/// typed by a stranger before opening, writing or running what they name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceMessage {
    /// The second launch's arguments, `argv[0]` included when it sent
    /// [`std::env::args_os`].
    pub args: Vec<OsString>,
    /// The second launch's working directory, against which its relative
    /// paths resolve. Empty when it could not read its own.
    pub cwd: PathBuf,
    /// The activation token its launcher gave it (`XDG_ACTIVATION_TOKEN` on
    /// Wayland, `DESKTOP_STARTUP_ID` on X11), if any.
    pub activation_token: Option<String>,
}

/// The primary's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Reply {
    /// The request was read and handed over.
    Accepted,
    /// The request's version is not the primary's, which is `version`.
    Rejected {
        /// The version the primary speaks.
        version: u8,
    },
}

/// Why a request could not be read.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum DecodeError {
    /// It names another protocol version: answer [`Reply::Rejected`].
    Version(u8),
    /// It is malformed or over a bound: close without answering.
    Malformed(&'static str),
}

/// Write one frame holding `payload`.
pub(super) fn write_frame(w: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    let len = u32::try_from(payload.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame over 4 GiB"))?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(payload)?;
    w.flush()
}

/// Read one frame of at most `max` bytes. A larger length is refused before
/// anything is allocated for it.
pub(super) fn read_frame(r: &mut impl Read, max: usize) -> io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame of {len} bytes is over the {max}-byte limit"),
        ));
    }
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload)?;
    Ok(payload)
}

/// Encode a request, or say which bound it is over.
pub(super) fn encode_request(
    args: &[OsString],
    cwd: &Path,
    activation_token: Option<&str>,
) -> io::Result<Vec<u8>> {
    let over = |what: String| io::Error::new(io::ErrorKind::InvalidInput, what);
    if args.len() > MAX_ARGS {
        return Err(over(format!(
            "{} arguments, over the limit of {MAX_ARGS}",
            args.len()
        )));
    }
    if let Some(token) = activation_token
        && token.len() > MAX_TOKEN
    {
        return Err(over(format!(
            "an activation token of {} bytes, over the limit of {MAX_TOKEN}",
            token.len()
        )));
    }
    let mut out = vec![VERSION];
    out.extend_from_slice(&(args.len() as u32).to_le_bytes());
    for arg in args {
        push_bytes(&mut out, &os_bytes(arg)?);
    }
    push_bytes(&mut out, &os_bytes(cwd.as_os_str())?);
    match activation_token {
        Some(token) => {
            out.push(1);
            push_bytes(&mut out, token.as_bytes());
        }
        None => out.push(0),
    }
    if out.len() > MAX_REQUEST {
        return Err(over(format!(
            "a request of {} bytes, over the limit of {MAX_REQUEST}",
            out.len()
        )));
    }
    Ok(out)
}

/// Decode a request, refusing anything over a bound, truncated, or with
/// bytes left over.
pub(super) fn decode_request(bytes: &[u8]) -> Result<InstanceMessage, DecodeError> {
    if bytes.len() > MAX_REQUEST {
        return Err(DecodeError::Malformed("over the size limit"));
    }
    let mut cursor = Cursor(bytes);
    let version = cursor.u8()?;
    if version != VERSION {
        return Err(DecodeError::Version(version));
    }
    let count = cursor.u32()? as usize;
    if count > MAX_ARGS {
        return Err(DecodeError::Malformed("too many arguments"));
    }
    // Each argument takes at least its 4-byte length, so the bytes left
    // bound the count, not the count the sender claims.
    let mut args = Vec::with_capacity(count.min(cursor.0.len() / 4));
    for _ in 0..count {
        args.push(os_string(cursor.bytes()?)?);
    }
    let cwd = PathBuf::from(os_string(cursor.bytes()?)?);
    let activation_token = match cursor.u8()? {
        0 => None,
        1 => {
            let token = cursor.bytes()?;
            if token.len() > MAX_TOKEN {
                return Err(DecodeError::Malformed("activation token too long"));
            }
            Some(
                String::from_utf8(token.to_vec())
                    .map_err(|_| DecodeError::Malformed("activation token not UTF-8"))?,
            )
        }
        _ => return Err(DecodeError::Malformed("bad activation token flag")),
    };
    if !cursor.0.is_empty() {
        return Err(DecodeError::Malformed("bytes after the request"));
    }
    Ok(InstanceMessage {
        args,
        cwd,
        activation_token,
    })
}

/// Encode a reply.
pub(super) fn encode_reply(reply: Reply) -> Vec<u8> {
    match reply {
        Reply::Accepted => vec![0],
        Reply::Rejected { version } => vec![1, version],
    }
}

/// Decode a reply.
pub(super) fn decode_reply(bytes: &[u8]) -> io::Result<Reply> {
    match bytes {
        [0] => Ok(Reply::Accepted),
        [1, version] => Ok(Reply::Rejected { version: *version }),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the running instance answered something unreadable",
        )),
    }
}

fn push_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(bytes);
}

/// Reads fields off the front of a request, each checked against what is
/// left before anything is allocated for it.
struct Cursor<'a>(&'a [u8]);

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        if n > self.0.len() {
            return Err(DecodeError::Malformed("truncated"));
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, DecodeError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn bytes(&mut self) -> Result<&'a [u8], DecodeError> {
        let len = self.u32()? as usize;
        self.take(len)
    }
}

#[cfg(unix)]
fn os_bytes(s: &OsStr) -> io::Result<Cow<'_, [u8]>> {
    use std::os::unix::ffi::OsStrExt;
    Ok(Cow::Borrowed(s.as_bytes()))
}

#[cfg(windows)]
fn os_bytes(s: &OsStr) -> io::Result<Cow<'_, [u8]>> {
    use std::os::windows::ffi::OsStrExt;
    Ok(Cow::Owned(utf16le_bytes(s.encode_wide())))
}

#[cfg(not(any(unix, windows)))]
fn os_bytes(s: &OsStr) -> io::Result<Cow<'_, [u8]>> {
    s.to_str()
        .map(|s| Cow::Borrowed(s.as_bytes()))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "an argument that is not Unicode",
            )
        })
}

#[cfg(unix)]
fn os_string(bytes: &[u8]) -> Result<OsString, DecodeError> {
    use std::os::unix::ffi::OsStringExt;
    Ok(OsString::from_vec(bytes.to_vec()))
}

#[cfg(windows)]
fn os_string(bytes: &[u8]) -> Result<OsString, DecodeError> {
    use std::os::windows::ffi::OsStringExt;
    utf16le_units(bytes)
        .map(|units| OsString::from_wide(&units))
        .ok_or(DecodeError::Malformed(
            "an argument of an odd number of bytes",
        ))
}

#[cfg(not(any(unix, windows)))]
fn os_string(bytes: &[u8]) -> Result<OsString, DecodeError> {
    String::from_utf8(bytes.to_vec())
        .map(OsString::from)
        .map_err(|_| DecodeError::Malformed("an argument that is not UTF-8"))
}

/// UTF-16 code units as the bytes Windows strings travel as. Any `u16`
/// goes, a lone surrogate included: no validation, so nothing is lost.
#[cfg(any(windows, test))]
fn utf16le_bytes(units: impl IntoIterator<Item = u16>) -> Vec<u8> {
    units.into_iter().flat_map(u16::to_le_bytes).collect()
}

/// The inverse of [`utf16le_bytes`]: `None` for an odd number of bytes.
#[cfg(any(windows, test))]
fn utf16le_units(bytes: &[u8]) -> Option<Vec<u16>> {
    let (pairs, rest) = bytes.as_chunks::<2>();
    if !rest.is_empty() {
        return None;
    }
    Some(pairs.iter().map(|&pair| u16::from_le_bytes(pair)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn a_request_round_trips_with_and_without_a_token() {
        for token in [None, Some("wayland-token-42")] {
            let bytes = encode_request(
                &args(&["app", "--open", "a b.txt"]),
                Path::new("/home/u"),
                token,
            )
            .unwrap();
            let message = decode_request(&bytes).unwrap();
            assert_eq!(message.args, args(&["app", "--open", "a b.txt"]));
            assert_eq!(message.cwd, Path::new("/home/u"));
            assert_eq!(message.activation_token.as_deref(), token);
        }
    }

    #[test]
    fn an_empty_request_round_trips() {
        let bytes = encode_request(&[], Path::new(""), None).unwrap();
        let message = decode_request(&bytes).unwrap();
        assert!(message.args.is_empty());
        assert_eq!(message.cwd, Path::new(""));
        assert_eq!(message.activation_token, None);
    }

    #[cfg(unix)]
    #[test]
    fn arguments_that_are_not_utf8_arrive_unchanged() {
        use std::os::unix::ffi::OsStringExt;
        let raw = OsString::from_vec(vec![b'f', 0xff, 0xfe, b'.', b't']);
        let bytes = encode_request(std::slice::from_ref(&raw), Path::new("/"), None).unwrap();
        assert_eq!(decode_request(&bytes).unwrap().args, vec![raw]);
    }

    /// The Windows encoding, checked on every platform: a lone surrogate
    /// and a pair both survive, low byte first, and an odd length is refused.
    #[test]
    fn utf16_code_units_round_trip_as_little_endian_bytes() {
        let units = [0x0066, 0xd800, 0x002e, 0xd83d, 0xde00, 0xdc00, 0xffff, 0];
        let bytes = utf16le_bytes(units);
        assert_eq!(&bytes[..4], &[0x66, 0x00, 0x00, 0xd8]);
        assert_eq!(bytes.len(), 2 * units.len());
        assert_eq!(utf16le_units(&bytes).unwrap(), units);
        assert_eq!(utf16le_units(&[]).unwrap(), Vec::<u16>::new());
        assert_eq!(utf16le_units(&bytes[..3]), None);
    }

    #[cfg(windows)]
    #[test]
    fn arguments_that_are_not_unicode_arrive_unchanged() {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        // An unpaired high surrogate, an unpaired low one, and a pair.
        let raw = OsString::from_wide(&[0x66, 0xd800, 0x2e, 0xdc00, 0xd83d, 0xde00]);
        assert!(raw.to_str().is_none());
        let cwd = OsString::from_wide(&[0x43, 0x3a, 0x5c, 0xdfff]);
        let bytes =
            encode_request(std::slice::from_ref(&raw), Path::new(&cwd), Some("tok")).unwrap();
        let message = decode_request(&bytes).unwrap();
        assert_eq!(message.args, vec![raw.clone()]);
        assert_eq!(
            message.args[0].encode_wide().collect::<Vec<_>>(),
            raw.encode_wide().collect::<Vec<_>>()
        );
        assert_eq!(message.cwd.as_os_str(), cwd.as_os_str());
    }

    #[cfg(windows)]
    #[test]
    fn an_argument_of_an_odd_number_of_bytes_is_malformed() {
        let mut bytes = vec![VERSION];
        bytes.extend_from_slice(&1u32.to_le_bytes());
        push_bytes(&mut bytes, &[0x41, 0x00, 0x42]);
        push_bytes(&mut bytes, &[]);
        bytes.push(0);
        assert!(matches!(
            decode_request(&bytes),
            Err(DecodeError::Malformed(_))
        ));
    }

    #[test]
    fn the_sender_refuses_every_bound() {
        let too_many = vec![OsString::from("x"); MAX_ARGS + 1];
        let err = encode_request(&too_many, Path::new("/"), None).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);

        let as_many = vec![OsString::from("x"); MAX_ARGS];
        let bytes = encode_request(&as_many, Path::new("/"), None).unwrap();
        assert_eq!(decode_request(&bytes).unwrap().args.len(), MAX_ARGS);

        let token = "t".repeat(MAX_TOKEN + 1);
        let err = encode_request(&[], Path::new("/"), Some(&token)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(encode_request(&[], Path::new("/"), Some(&token[1..])).is_ok());

        let big = vec![OsString::from("y".repeat(MAX_REQUEST))];
        let err = encode_request(&big, Path::new("/"), None).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn another_version_is_reported_as_such() {
        let mut bytes = encode_request(&args(&["a"]), Path::new("/"), None).unwrap();
        bytes[0] = VERSION + 1;
        assert_eq!(
            decode_request(&bytes),
            Err(DecodeError::Version(VERSION + 1))
        );
        // Only the first byte is read: a later protocol may change the rest.
        assert_eq!(
            decode_request(&[VERSION + 1]),
            Err(DecodeError::Version(VERSION + 1))
        );
    }

    #[test]
    fn the_receiver_refuses_every_bound_and_every_malformation() {
        let malformed =
            |bytes: &[u8]| matches!(decode_request(bytes), Err(DecodeError::Malformed(_)));

        assert!(malformed(&[]), "empty");

        let mut too_many = vec![VERSION];
        too_many.extend_from_slice(&((MAX_ARGS + 1) as u32).to_le_bytes());
        assert!(malformed(&too_many), "an argument count over the limit");

        let mut huge_arg = vec![VERSION, 1, 0, 0, 0];
        huge_arg.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(malformed(&huge_arg), "an argument longer than what is left");

        let good = encode_request(&args(&["a", "b"]), Path::new("/x"), Some("tok")).unwrap();
        for cut in 0..good.len() {
            assert!(malformed(&good[..cut]), "truncated at {cut}");
        }

        let mut trailing = good.clone();
        trailing.push(0);
        assert!(malformed(&trailing), "a byte after the request");

        let mut flag = encode_request(&[], Path::new("/"), None).unwrap();
        *flag.last_mut().unwrap() = 2;
        assert!(malformed(&flag), "a token flag that is neither 0 nor 1");

        let mut long_token = encode_request(&[], Path::new("/"), None).unwrap();
        *long_token.last_mut().unwrap() = 1;
        push_bytes(&mut long_token, &vec![b't'; MAX_TOKEN + 1]);
        assert!(malformed(&long_token), "a token over the limit");

        let mut bad_token = encode_request(&[], Path::new("/"), None).unwrap();
        *bad_token.last_mut().unwrap() = 1;
        push_bytes(&mut bad_token, &[0xff]);
        assert!(malformed(&bad_token), "a token that is not UTF-8");

        assert!(
            malformed(&vec![VERSION; MAX_REQUEST + 1]),
            "over the size limit"
        );
    }

    #[test]
    fn replies_round_trip_and_nothing_else_is_read_as_one() {
        for reply in [Reply::Accepted, Reply::Rejected { version: 7 }] {
            assert_eq!(decode_reply(&encode_reply(reply)).unwrap(), reply);
        }
        for bad in [&[][..], &[2], &[0, 0], &[1], &[1, 2, 3]] {
            assert_eq!(
                decode_reply(bad).unwrap_err().kind(),
                io::ErrorKind::InvalidData,
                "{bad:?}"
            );
        }
        assert!(encode_reply(Reply::Rejected { version: 9 }).len() <= MAX_REPLY);
    }

    #[test]
    fn frames_round_trip_and_an_oversized_length_is_refused_before_allocating() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"hello").unwrap();
        let mut r = io::Cursor::new(buf);
        assert_eq!(read_frame(&mut r, 5).unwrap(), b"hello");

        let mut buf = Vec::new();
        write_frame(&mut buf, b"hello").unwrap();
        let err = read_frame(&mut io::Cursor::new(buf), 4).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);

        let hostile = u32::MAX.to_le_bytes().to_vec();
        let err = read_frame(&mut io::Cursor::new(hostile), MAX_REQUEST).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);

        let mut truncated = Vec::new();
        write_frame(&mut truncated, b"0123456789").unwrap();
        truncated.truncate(8);
        let err = read_frame(&mut io::Cursor::new(truncated), MAX_REQUEST).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }
}
