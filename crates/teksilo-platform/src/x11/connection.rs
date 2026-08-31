// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! A private X11 connection, plus the atom cache and property helpers the
//! title-bar probe and the XDND backend both need.
//!
//! # Why a private connection
//!
//! winit owns its own X11 connection and pumps it from its event loop. X11 has
//! no equivalent of libwayland's multi-queue model — two connection objects
//! wrapping one file descriptor would race on sequence numbers and
//! reply/event demultiplexing — so we never touch winit's. We open our own
//! [`RustConnection`] (pure Rust, no `libxcb` linkage, no `unsafe`) and use the
//! raw window handle only to learn which XID to talk *about*.
//!
//! This is what makes the `XdndProxy` indirection necessary on the inbound
//! path: XDND `ClientMessage`s are sent with an empty event mask, which the X
//! protocol delivers only to the client that *created* the destination window
//! — winit, not us. See `crate::external_dnd::x11`.

use std::cell::RefCell;
use std::collections::VecDeque;

use x11rb::connection::Connection;
use x11rb::errors::{ConnectError, ConnectionError, ReplyError};
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt as _, EventMask, PropMode, Timestamp, Window,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

/// Anything that can go wrong talking to the X server.
#[derive(Debug, thiserror::Error)]
pub enum X11Error {
    #[error("could not connect to the X server: {0}")]
    Connect(#[from] ConnectError),
    #[error("X11 connection error: {0}")]
    Connection(#[from] ConnectionError),
    #[error("X11 request failed: {0}")]
    Reply(#[from] ReplyError),
    #[error("could not allocate an X11 resource id: {0}")]
    IdAllocation(#[from] x11rb::errors::ReplyOrIdError),
    #[error("timed out waiting for {0}")]
    Timeout(&'static str),
}

/// Fire a void request and discard both failure modes.
///
/// Used for best-effort operations — teardown, replies to a peer that may have
/// already exited — where a `BadWindow` from a window that vanished mid-drag is
/// the expected case, not an error worth propagating.
pub fn ignore_errors<C: x11rb::connection::RequestConnection>(
    result: Result<x11rb::cookie::VoidCookie<'_, C>, ConnectionError>,
) {
    if let Ok(cookie) = result {
        let _ = cookie.check();
    }
}

/// The interned atoms used by the title-bar probe and the XDND backend.
///
/// Interned once per connection: an X round trip per atom per use would put a
/// server round trip in the middle of every drag motion.
#[derive(Debug, Clone)]
pub struct Atoms {
    // --- XDND protocol ---
    pub xdnd_aware: Atom,
    pub xdnd_proxy: Atom,
    pub xdnd_selection: Atom,
    pub xdnd_enter: Atom,
    pub xdnd_position: Atom,
    pub xdnd_status: Atom,
    pub xdnd_leave: Atom,
    pub xdnd_drop: Atom,
    pub xdnd_finished: Atom,
    pub xdnd_type_list: Atom,
    pub xdnd_action_copy: Atom,
    pub xdnd_action_move: Atom,
    pub xdnd_action_link: Atom,
    pub xdnd_action_private: Atom,
    pub xdnd_action_list: Atom,

    // --- selection transfer ---
    pub incr: Atom,
    pub targets: Atom,
    pub timestamp: Atom,
    /// The property we ask sources to write converted selection data into.
    /// Namespaced so it cannot collide with a toolkit's own scratch property.
    pub teksilo_transfer: Atom,
    /// Scratch property for the server-timestamp round trip.
    ///
    /// Deliberately **not** [`Self::teksilo_transfer`]: the timestamp trick
    /// appends to the property with type `STRING`, which would hit `BadMatch`
    /// against an in-flight `INCR` chunk of a different type, and its
    /// `PropertyNotify` would drive the INCR reader to consume the property out
    /// from under the running transfer.
    pub teksilo_timestamp: Atom,

    // --- MIME types we exchange ---
    pub text_uri_list: Atom,
    pub text_plain_utf8: Atom,
    pub text_plain: Atom,
    pub utf8_string: Atom,
    pub string: Atom,

    // --- EWMH / Motif (custom title bar) ---
    pub net_supported: Atom,
    pub net_supporting_wm_check: Atom,
    pub net_wm_moveresize: Atom,
    pub motif_wm_hints: Atom,
}

impl Atoms {
    fn intern(conn: &RustConnection) -> Result<Self, X11Error> {
        // Fire every InternAtom request before reading any reply, so the whole
        // set costs one round trip instead of ~30.
        const NAMES: &[&[u8]] = &[
            b"XdndAware",
            b"XdndProxy",
            b"XdndSelection",
            b"XdndEnter",
            b"XdndPosition",
            b"XdndStatus",
            b"XdndLeave",
            b"XdndDrop",
            b"XdndFinished",
            b"XdndTypeList",
            b"XdndActionCopy",
            b"XdndActionMove",
            b"XdndActionLink",
            b"XdndActionPrivate",
            b"XdndActionList",
            b"INCR",
            b"TARGETS",
            b"TIMESTAMP",
            b"_TEKSILO_DND_TRANSFER",
            b"_TEKSILO_DND_TIMESTAMP",
            b"text/uri-list",
            b"text/plain;charset=utf-8",
            b"text/plain",
            b"UTF8_STRING",
            b"STRING",
            b"_NET_SUPPORTED",
            b"_NET_SUPPORTING_WM_CHECK",
            b"_NET_WM_MOVERESIZE",
            b"_MOTIF_WM_HINTS",
        ];

        let cookies = NAMES
            .iter()
            .map(|name| conn.intern_atom(false, name))
            .collect::<Result<Vec<_>, _>>()?;
        let mut atoms = Vec::with_capacity(cookies.len());
        for cookie in cookies {
            atoms.push(cookie.reply()?.atom);
        }
        let mut next = atoms.into_iter();
        let mut take = || next.next().expect("one atom per interned name");

        Ok(Self {
            xdnd_aware: take(),
            xdnd_proxy: take(),
            xdnd_selection: take(),
            xdnd_enter: take(),
            xdnd_position: take(),
            xdnd_status: take(),
            xdnd_leave: take(),
            xdnd_drop: take(),
            xdnd_finished: take(),
            xdnd_type_list: take(),
            xdnd_action_copy: take(),
            xdnd_action_move: take(),
            xdnd_action_link: take(),
            xdnd_action_private: take(),
            xdnd_action_list: take(),
            incr: take(),
            targets: take(),
            timestamp: take(),
            teksilo_transfer: take(),
            teksilo_timestamp: take(),
            text_uri_list: take(),
            text_plain_utf8: take(),
            text_plain: take(),
            utf8_string: take(),
            string: take(),
            net_supported: take(),
            net_supporting_wm_check: take(),
            net_wm_moveresize: take(),
            motif_wm_hints: take(),
        })
    }

    /// The MIME target atoms we accept from a drop source, most preferred
    /// first. `text/uri-list` leads because it is the only one that yields
    /// real file paths.
    pub fn preferred_targets(&self) -> [Atom; 5] {
        [
            self.text_uri_list,
            self.text_plain_utf8,
            self.utf8_string,
            self.text_plain,
            self.string,
        ]
    }

    /// Map a MIME type string to its atom, for the atoms we know natively.
    /// Unknown types are interned on demand by the caller.
    pub fn atom_for_mime(&self, mime: &str) -> Option<Atom> {
        match mime {
            "text/uri-list" => Some(self.text_uri_list),
            "text/plain;charset=utf-8" => Some(self.text_plain_utf8),
            "text/plain" => Some(self.text_plain),
            "UTF8_STRING" => Some(self.utf8_string),
            "STRING" => Some(self.string),
            _ => None,
        }
    }
}

/// A property read back from the server, with its type and format preserved so
/// the caller can validate what it got.
#[derive(Debug, Clone)]
pub struct PropertyValue {
    pub type_: Atom,
    pub format: u8,
    pub bytes: Vec<u8>,
}

impl PropertyValue {
    /// Reinterpret a 32-bit-format property as `u32`s. Returns an empty vector
    /// for any other format, so a malformed property degrades to "absent"
    /// rather than to garbage values.
    pub fn as_u32s(&self) -> Vec<u32> {
        if self.format != 32 {
            return Vec::new();
        }
        self.bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| u32::from_ne_bytes(*chunk))
            .collect()
    }

    /// The single `u32` of a scalar 32-bit property, if that is what it is.
    pub fn as_u32(&self) -> Option<u32> {
        self.as_u32s().first().copied()
    }
}

/// Our own connection to the X server, with atoms and an event pushback queue.
///
/// Not `Sync`: it holds a pushback buffer behind a `RefCell` and is used from a
/// single thread (the per-window DnD thread, or transiently the main thread for
/// the EWMH probe).
pub struct X11Connection {
    conn: RustConnection,
    root: Window,
    atoms: Atoms,
    /// Events pulled off the wire while waiting for a specific reply, to be
    /// handed back to the main loop in order. Without this, the
    /// server-timestamp round trip below would silently swallow a
    /// `ClientMessage` that arrived at the same moment.
    pending: RefCell<VecDeque<Event>>,
}

impl X11Connection {
    /// Open a fresh connection to the display named by `$DISPLAY`.
    pub fn open() -> Result<Self, X11Error> {
        let (conn, screen_num) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen_num].root;
        let atoms = Atoms::intern(&conn)?;
        Ok(Self {
            conn,
            root,
            atoms,
            pending: RefCell::new(VecDeque::new()),
        })
    }

    pub fn conn(&self) -> &RustConnection {
        &self.conn
    }

    pub fn root(&self) -> Window {
        self.root
    }

    pub fn atoms(&self) -> &Atoms {
        &self.atoms
    }

    pub fn flush(&self) -> Result<(), X11Error> {
        self.conn.flush()?;
        Ok(())
    }

    /// Read a whole property, following `bytes_after` until the server has no
    /// more to give.
    ///
    /// A single `GetProperty` is capped by `long_length`; properties that
    /// exceed it (a long `text/uri-list`, a big `_NET_SUPPORTED`) come back
    /// truncated with `bytes_after > 0`. Reading only the first chunk is a
    /// classic source of "the last few files vanished" bugs.
    pub fn get_property_full(
        &self,
        window: Window,
        property: Atom,
        type_: Atom,
    ) -> Result<Option<PropertyValue>, X11Error> {
        // 4 KiB of 32-bit units per round trip.
        const CHUNK_UNITS: u32 = 1024;

        let mut offset = 0u32;
        let mut out: Option<PropertyValue> = None;
        loop {
            let reply = self
                .conn
                .get_property(false, window, property, type_, offset, CHUNK_UNITS)?
                .reply()?;
            if reply.type_ == x11rb::NONE {
                return Ok(out);
            }
            let more = reply.bytes_after > 0;
            let format = reply.format;
            let reply_type = reply.type_;
            let len = reply.value.len();
            match &mut out {
                Some(acc) => acc.bytes.extend_from_slice(&reply.value),
                None => {
                    out = Some(PropertyValue {
                        type_: reply_type,
                        format,
                        bytes: reply.value,
                    })
                }
            }
            if !more || len == 0 {
                return Ok(out);
            }
            // `long_offset` counts 32-bit units, whatever the actual format.
            offset += (len as u32).div_ceil(4);
        }
    }

    /// Read a property and delete it in the same request — the ICCCM idiom for
    /// selection transfers. Deleting is what tells an `INCR` sender we are
    /// ready for the next chunk, so the read and the delete must be atomic.
    pub fn get_property_and_delete(
        &self,
        window: Window,
        property: Atom,
    ) -> Result<Option<PropertyValue>, X11Error> {
        // Ask for everything in one go: the server caps the reply at
        // `long_length` units and reports the rest via `bytes_after`, but an
        // INCR chunk is sized to fit a single request by construction.
        let reply = self
            .conn
            .get_property(true, window, property, AtomEnum::ANY, 0, u32::MAX / 4)?
            .reply()?;
        if reply.type_ == x11rb::NONE {
            return Ok(None);
        }
        Ok(Some(PropertyValue {
            type_: reply.type_,
            format: reply.format,
            bytes: reply.value,
        }))
    }

    /// Write a 32-bit property.
    pub fn set_property32(
        &self,
        window: Window,
        property: Atom,
        type_: Atom,
        data: &[u32],
    ) -> Result<(), X11Error> {
        self.conn
            .change_property32(PropMode::REPLACE, window, property, type_, data)?
            .check()?;
        Ok(())
    }

    /// Write an 8-bit property.
    pub fn set_property8(
        &self,
        window: Window,
        property: Atom,
        type_: Atom,
        data: &[u8],
    ) -> Result<(), X11Error> {
        self.conn
            .change_property8(PropMode::REPLACE, window, property, type_, data)?
            .check()?;
        Ok(())
    }

    /// Obtain a real server timestamp.
    ///
    /// The ICCCM-sanctioned trick: append **zero** bytes to a property on a
    /// window we own, which changes nothing but still makes the server emit a
    /// `PropertyNotify` stamped with the current time. `CurrentTime` is not a
    /// substitute — XDND and selection ownership both need a comparable
    /// timestamp so a stale request can be told from a fresh one.
    ///
    /// `window` must have `PROPERTY_CHANGE` selected. Events seen while
    /// waiting are pushed back for the caller's loop, in order.
    pub fn fetch_timestamp(&self, window: Window) -> Result<Timestamp, X11Error> {
        self.conn
            .change_property8(
                PropMode::APPEND,
                window,
                self.atoms.teksilo_timestamp,
                AtomEnum::STRING,
                &[],
            )?
            .check()?;
        self.conn.flush()?;

        // Bounded so a server that never answers cannot wedge the thread.
        for _ in 0..64 {
            let event = self.conn.wait_for_event()?;
            if let Event::PropertyNotify(ref notify) = event
                && notify.window == window
                && notify.atom == self.atoms.teksilo_timestamp
            {
                return Ok(notify.time);
            }
            self.pending.borrow_mut().push_back(event);
        }
        Err(X11Error::Timeout(
            "a PropertyNotify carrying a server timestamp",
        ))
    }

    /// Next event, draining anything [`Self::fetch_timestamp`] pushed back
    /// first. Blocks.
    pub fn next_event(&self) -> Result<Event, X11Error> {
        if let Some(event) = self.pending.borrow_mut().pop_front() {
            return Ok(event);
        }
        Ok(self.conn.wait_for_event()?)
    }

    /// Next event if one is already available, else `None`. Never blocks.
    pub fn poll_event(&self) -> Result<Option<Event>, X11Error> {
        if let Some(event) = self.pending.borrow_mut().pop_front() {
            return Ok(Some(event));
        }
        Ok(self.conn.poll_for_event()?)
    }

    /// Send a 32-bit `ClientMessage`.
    ///
    /// XDND and `_NET_WM_MOVERESIZE` both specify `propagate = false`. The
    /// event mask differs: XDND messages go to a specific client with an empty
    /// mask (the X protocol then delivers to that window's *creator*), whereas
    /// root-window messages must carry the substructure masks so the window
    /// manager sees them.
    pub fn send_client_message(
        &self,
        destination: Window,
        window_field: Window,
        type_: Atom,
        data: [u32; 5],
        mask: EventMask,
    ) -> Result<(), X11Error> {
        use x11rb::protocol::xproto::ClientMessageEvent;

        let event = ClientMessageEvent::new(32, window_field, type_, data);
        self.conn
            .send_event(false, destination, mask, event)?
            .check()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_value_reads_32_bit_words() {
        let value = PropertyValue {
            type_: 1,
            format: 32,
            bytes: 5u32
                .to_ne_bytes()
                .into_iter()
                .chain(7u32.to_ne_bytes())
                .collect(),
        };
        assert_eq!(value.as_u32s(), vec![5, 7]);
        assert_eq!(value.as_u32(), Some(5));
    }

    #[test]
    fn property_value_rejects_a_mismatched_format() {
        // A window id claimed to be 8-bit is a malformed property; reading it
        // as words would produce plausible-looking garbage, so we read nothing.
        let value = PropertyValue {
            type_: 1,
            format: 8,
            bytes: vec![1, 2, 3, 4],
        };
        assert!(value.as_u32s().is_empty());
        assert_eq!(value.as_u32(), None);
    }

    #[test]
    fn property_value_ignores_a_trailing_partial_word() {
        let value = PropertyValue {
            type_: 1,
            format: 32,
            bytes: vec![1, 2, 3, 4, 5],
        };
        assert_eq!(value.as_u32s().len(), 1);
    }
}
