// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! XDND wire protocol — pure functions, no X connection.
//!
//! Everything here is deliberately connection-free so the protocol can be
//! tested exhaustively in `cargo test` on any machine, with no display server.
//! `crate::external_dnd::x11` holds the I/O that drives it.
//!
//! Reference: "Drag-and-Drop Protocol for the X Window System", version 5
//! (<https://freedesktop.org/wiki/Specifications/XDND/>). Field layouts below
//! were cross-checked against Paul Sheer's reference `xdnd.c`, GTK's
//! `gdkdnd-x11.c`, and Qt's `qxcbdrag.cpp`, which agree on every field.
//!
//! # Coordinate packing
//!
//! `XdndPosition` and `XdndStatus` pack a root-window point into a single
//! 32-bit word as `(x << 16) | (y & 0xFFFF)`. The halves are **signed 16-bit**,
//! so a virtual root wider or taller than ±32767 px wraps. That is a limit of
//! the wire format, not of this implementation; there is no protocol-level fix.

/// Highest XDND version this implementation speaks, advertised in `XdndAware`.
pub const XDND_VERSION: u32 = 5;

/// Lowest source version we will talk to. Versions below 3 put `XdndAware` on
/// subwindows and predate the timestamp fields; no live toolkit still emits
/// them. GTK and Qt both draw the line here too.
pub const MIN_SUPPORTED_VERSION: u32 = 3;

/// `XdndStatus.data[1]` bit 0 — the target will accept a drop here.
const STATUS_ACCEPT: u32 = 1;
/// `XdndStatus.data[1]` bit 1 — send `XdndPosition` again even inside the
/// rectangle in `data[2..3]`.
const STATUS_WANT_POSITION: u32 = 2;

/// `XdndEnter.data[1]` bit 0 — the source offers more than three types, so the
/// full list must be read from its `XdndTypeList` property.
const ENTER_MORE_TYPES: u32 = 1;

// ============================================================
// Coordinate packing
// ============================================================

/// Pack a root-relative point into the single 32-bit word XDND uses.
pub fn pack_coords(x: i16, y: i16) -> u32 {
    ((x as u16 as u32) << 16) | (y as u16 as u32)
}

/// Unpack the point packed by [`pack_coords`]. Both halves are signed, so a
/// monitor placed left of / above the primary (negative root coordinates)
/// round-trips correctly.
pub fn unpack_coords(packed: u32) -> (i16, i16) {
    (
        ((packed >> 16) & 0xFFFF) as u16 as i16,
        (packed & 0xFFFF) as u16 as i16,
    )
}

// ============================================================
// Version negotiation
// ============================================================

/// Resolve the protocol version to speak with a peer advertising `advertised`.
///
/// The effective version is `min(ours, theirs)`; a peer below
/// [`MIN_SUPPORTED_VERSION`] is refused outright (`None`).
pub fn negotiate_version(advertised: u32) -> Option<u32> {
    if advertised < MIN_SUPPORTED_VERSION {
        return None;
    }
    Some(advertised.min(XDND_VERSION))
}

/// Extract the version a source put in the high byte of `XdndEnter.data[1]`.
pub fn enter_version(data1: u32) -> u32 {
    data1 >> 24
}

// ============================================================
// XdndProxy validation
// ============================================================

/// Resolve a target window's `XdndProxy`, applying the spec's crash-recovery
/// rule.
///
/// `window_proxy` is `XdndProxy` read from the candidate window, `proxy_proxy`
/// is `XdndProxy` read from the window that first value points at. The spec
/// requires the proxy to point at *itself*; if it does not — or the proxy
/// window is gone — the property is stale (left over from a crash) and must be
/// ignored, falling back to the original window.
///
/// GTK (`xdnd_check_dest`) and Qt (`xdndProxy`) both implement exactly this.
pub fn resolve_proxy(window: u32, window_proxy: Option<u32>, proxy_proxy: Option<u32>) -> u32 {
    match window_proxy {
        Some(proxy) if proxy != 0 && proxy_proxy == Some(proxy) => proxy,
        _ => window,
    }
}

// ============================================================
// Message encoding / decoding
// ============================================================

/// A decoded `XdndEnter`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enter {
    /// The source window, which owns `XdndSelection` and receives our replies.
    pub source: u32,
    /// Negotiated protocol version, already clamped to ours.
    pub version: u32,
    /// The (up to three) type atoms carried inline. Empty slots are dropped.
    pub types: Vec<u32>,
    /// When set, `types` is only a prefix — read the source's `XdndTypeList`.
    pub more_types: bool,
}

/// Decode `XdndEnter`. Returns `None` when the source advertises a version we
/// refuse (see [`negotiate_version`]).
pub fn decode_enter(data: [u32; 5]) -> Option<Enter> {
    let version = negotiate_version(enter_version(data[1]))?;
    let types = data[2..5]
        .iter()
        .copied()
        .filter(|&atom| atom != 0)
        .collect();
    Some(Enter {
        source: data[0],
        version,
        types,
        more_types: data[1] & ENTER_MORE_TYPES != 0,
    })
}

/// Encode `XdndEnter` for the source side. `types` may hold any number of
/// atoms; only the first three travel inline and the "more types" bit is set
/// automatically when there are more (the rest go in `XdndTypeList`).
pub fn encode_enter(source: u32, version: u32, types: &[u32]) -> [u32; 5] {
    let mut data = [0u32; 5];
    data[0] = source;
    data[1] = version << 24;
    if types.len() > 3 {
        data[1] |= ENTER_MORE_TYPES;
    }
    for (slot, atom) in data[2..5].iter_mut().zip(types.iter().copied()) {
        *slot = atom;
    }
    data
}

/// A decoded `XdndPosition`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    pub source: u32,
    /// Pointer position in **root** coordinates, physical pixels.
    pub root_x: i16,
    pub root_y: i16,
    /// Timestamp to quote in the eventual `ConvertSelection`.
    pub time: u32,
    /// The action the source proposes.
    pub action: u32,
}

/// Decode `XdndPosition`.
pub fn decode_position(data: [u32; 5]) -> Position {
    let (root_x, root_y) = unpack_coords(data[2]);
    Position {
        source: data[0],
        root_x,
        root_y,
        time: data[3],
        action: data[4],
    }
}

/// Encode `XdndPosition` for the source side.
pub fn encode_position(source: u32, root_x: i16, root_y: i16, time: u32, action: u32) -> [u32; 5] {
    [source, 0, pack_coords(root_x, root_y), time, action]
}

/// Encode `XdndStatus` for the target side.
///
/// The "no-resend rectangle" is always sent **empty**, which the spec defines
/// as "send another message when the mouse moves". A drop target that hit-tests
/// per-widget cannot describe its accept region as one rectangle, so
/// suppressing position updates would make hover feedback wrong; the cost is
/// one small message per motion event.
pub fn encode_status(target: u32, accept: bool, action: u32) -> [u32; 5] {
    let mut flags = STATUS_WANT_POSITION;
    if accept {
        flags |= STATUS_ACCEPT;
    }
    [target, flags, 0, 0, if accept { action } else { 0 }]
}

/// A decoded `XdndStatus` (source side).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub target: u32,
    pub accepted: bool,
    /// The action the target would perform. Meaningful from version 2 on;
    /// zero (`None` atom) when the target is not accepting.
    pub action: u32,
}

/// Decode `XdndStatus`.
pub fn decode_status(data: [u32; 5]) -> Status {
    Status {
        target: data[0],
        accepted: data[1] & STATUS_ACCEPT != 0,
        action: data[4],
    }
}

/// Encode `XdndLeave`.
pub fn encode_leave(source: u32) -> [u32; 5] {
    [source, 0, 0, 0, 0]
}

/// Encode `XdndDrop`. `time` must be a real server timestamp — never
/// `CurrentTime` — so the source can reject a stale conversion request.
pub fn encode_drop(source: u32, time: u32) -> [u32; 5] {
    [source, 0, time, 0, 0]
}

/// A decoded `XdndDrop`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drop {
    pub source: u32,
    /// Timestamp to quote in `ConvertSelection`.
    pub time: u32,
}

/// Decode `XdndDrop`.
pub fn decode_drop(data: [u32; 5]) -> Drop {
    Drop {
        source: data[0],
        time: data[2],
    }
}

/// Encode `XdndFinished` for the target side.
///
/// The accepted flag and performed action in `data[1]`/`data[2]` are version-5
/// additions. Against an older source we must send zeroes there: a v3/v4 source
/// treats the drop as unconditionally accepted and reading our flags would be
/// out of contract.
pub fn encode_finished(target: u32, version: u32, accepted: bool, action: u32) -> [u32; 5] {
    if version < 5 {
        return [target, 0, 0, 0, 0];
    }
    [
        target,
        u32::from(accepted),
        if accepted { action } else { 0 },
        0,
        0,
    ]
}

/// A decoded `XdndFinished` (source side).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    pub target: u32,
    /// Whether the target accepted **and** performed the action.
    ///
    /// Only version 5 reports this. Against an older target the bit is absent
    /// and the spec says to behave as v2–v4 always did — assume success — so
    /// `negotiated_version` decides how `data[1]` is read.
    pub accepted: bool,
    pub action: u32,
}

/// Decode `XdndFinished`, honouring the negotiated version.
pub fn decode_finished(data: [u32; 5], negotiated_version: u32) -> Finished {
    if negotiated_version < 5 {
        return Finished {
            target: data[0],
            accepted: true,
            action: 0,
        };
    }
    Finished {
        target: data[0],
        accepted: data[1] & 1 != 0,
        action: data[2],
    }
}

// ============================================================
// Type selection
// ============================================================

/// Pick the best type atom to request from `offered`, given our `preferred`
/// list in descending priority. Returns `None` when nothing matches.
pub fn choose_type(offered: &[u32], preferred: &[u32]) -> Option<u32> {
    preferred
        .iter()
        .copied()
        .find(|candidate| offered.contains(candidate))
}

// ============================================================
// INCR assembly
// ============================================================

/// Reassembles an ICCCM `INCR` selection transfer.
///
/// The sender writes the payload in chunks, each appearing as a property
/// change on our requestor window; we delete the property after each read,
/// which is the signal for the next chunk. A **zero-length** chunk terminates
/// the transfer.
#[derive(Debug, Default)]
pub struct IncrAssembler {
    buffer: Vec<u8>,
    complete: bool,
}

impl IncrAssembler {
    /// Start an assembly. `expected` is the sender's size hint from the `INCR`
    /// property; it is advisory (senders may over- or under-estimate) and used
    /// only to pre-allocate.
    pub fn new(expected: usize) -> Self {
        Self {
            // Cap the hint so a bogus/hostile size can't request a huge
            // allocation up front; the buffer still grows as data arrives.
            buffer: Vec::with_capacity(expected.min(1 << 20)),
            complete: false,
        }
    }

    /// Feed one chunk. Returns `true` once the terminating empty chunk has
    /// arrived and [`Self::finish`] is meaningful.
    pub fn push(&mut self, chunk: &[u8]) -> bool {
        if chunk.is_empty() {
            self.complete = true;
        } else {
            self.buffer.extend_from_slice(chunk);
        }
        self.complete
    }

    /// Whether the terminating chunk has been seen.
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// Take the assembled payload.
    pub fn finish(self) -> Vec<u8> {
        self.buffer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- coordinate packing ----------

    #[test]
    fn coords_round_trip_including_negatives() {
        for (x, y) in [
            (0, 0),
            (1920, 1080),
            (-1, -1),
            (-1920, 200),
            (i16::MIN, i16::MAX),
        ] {
            assert_eq!(unpack_coords(pack_coords(x, y)), (x, y), "({x}, {y})");
        }
    }

    #[test]
    fn coords_pack_x_into_the_high_half() {
        // The spec is explicit about which half is which; swapping them is the
        // classic XDND bug (drops land on the wrong widget along one axis).
        assert_eq!(pack_coords(0x1234, 0x5678), 0x1234_5678);
    }

    // ---------- version negotiation ----------

    #[test]
    fn negotiation_clamps_to_our_version() {
        assert_eq!(negotiate_version(5), Some(5));
        assert_eq!(
            negotiate_version(9),
            Some(5),
            "a newer peer must be clamped down"
        );
        assert_eq!(negotiate_version(3), Some(3));
        assert_eq!(negotiate_version(4), Some(4));
    }

    #[test]
    fn negotiation_refuses_prehistoric_versions() {
        assert_eq!(negotiate_version(2), None);
        assert_eq!(negotiate_version(0), None);
    }

    // ---------- XdndProxy ----------

    #[test]
    fn proxy_is_honoured_when_it_points_at_itself() {
        assert_eq!(resolve_proxy(0x100, Some(0x200), Some(0x200)), 0x200);
    }

    #[test]
    fn stale_proxy_falls_back_to_the_original_window() {
        // Left over after a crash: the proxy window is gone, so reading its own
        // XdndProxy yields nothing. The spec says ignore the property.
        assert_eq!(resolve_proxy(0x100, Some(0x200), None), 0x100);
        // Points somewhere else entirely — equally untrustworthy.
        assert_eq!(resolve_proxy(0x100, Some(0x200), Some(0x300)), 0x100);
        // Zero is not a window.
        assert_eq!(resolve_proxy(0x100, Some(0), Some(0)), 0x100);
    }

    #[test]
    fn no_proxy_property_means_use_the_window() {
        assert_eq!(resolve_proxy(0x100, None, None), 0x100);
    }

    // ---------- Enter ----------

    #[test]
    fn enter_round_trips_with_three_types() {
        let encoded = encode_enter(0xAB, 5, &[10, 20, 30]);
        let decoded = decode_enter(encoded).expect("v5 is supported");
        assert_eq!(
            decoded,
            Enter {
                source: 0xAB,
                version: 5,
                types: vec![10, 20, 30],
                more_types: false
            }
        );
    }

    #[test]
    fn enter_sets_the_more_types_bit_past_three() {
        let encoded = encode_enter(1, 5, &[10, 20, 30, 40]);
        let decoded = decode_enter(encoded).unwrap();
        assert!(decoded.more_types, "a fourth type must flag XdndTypeList");
        assert_eq!(decoded.types, vec![10, 20, 30], "only three travel inline");
    }

    #[test]
    fn enter_drops_empty_type_slots() {
        let decoded = decode_enter(encode_enter(1, 5, &[10])).unwrap();
        assert_eq!(decoded.types, vec![10], "None atoms are not types");
    }

    #[test]
    fn enter_from_an_ancient_source_is_refused() {
        let mut data = encode_enter(1, 5, &[10]);
        data[1] = (2 << 24) | (data[1] & 0x00FF_FFFF); // claim version 2
        assert!(decode_enter(data).is_none());
    }

    #[test]
    fn enter_from_a_newer_source_is_clamped() {
        let mut data = encode_enter(1, 5, &[10]);
        data[1] = (7 << 24) | (data[1] & 0x00FF_FFFF);
        assert_eq!(decode_enter(data).unwrap().version, 5);
    }

    // ---------- Position / Status ----------

    #[test]
    fn position_round_trips() {
        let decoded = decode_position(encode_position(0xAB, -300, 900, 12345, 77));
        assert_eq!(
            decoded,
            Position {
                source: 0xAB,
                root_x: -300,
                root_y: 900,
                time: 12345,
                action: 77
            }
        );
    }

    #[test]
    fn status_always_requests_further_position_messages() {
        // An empty rectangle plus the want-position bit: per-widget hit-testing
        // needs every motion, and one rectangle cannot describe it.
        let data = encode_status(0x10, true, 42);
        assert_eq!(data[2], 0, "rectangle origin must be empty");
        assert_eq!(data[3], 0, "rectangle extent must be empty");
        assert_ne!(data[1] & STATUS_WANT_POSITION, 0);
    }

    #[test]
    fn status_round_trips_accept_and_reject() {
        let accepted = decode_status(encode_status(0x10, true, 42));
        assert_eq!(
            accepted,
            Status {
                target: 0x10,
                accepted: true,
                action: 42
            }
        );

        let rejected = decode_status(encode_status(0x10, false, 42));
        assert!(!rejected.accepted);
        assert_eq!(
            rejected.action, 0,
            "a rejecting target must advertise no action"
        );
    }

    // ---------- Drop / Finished ----------

    #[test]
    fn drop_round_trips_its_timestamp() {
        assert_eq!(
            decode_drop(encode_drop(0xAB, 999)),
            Drop {
                source: 0xAB,
                time: 999
            }
        );
    }

    #[test]
    fn finished_reports_the_action_on_v5() {
        let decoded = decode_finished(encode_finished(0x10, 5, true, 42), 5);
        assert_eq!(
            decoded,
            Finished {
                target: 0x10,
                accepted: true,
                action: 42
            }
        );
    }

    #[test]
    fn finished_omits_v5_fields_for_older_peers() {
        // Writing our accept bit into a v3 exchange would be out of contract.
        let data = encode_finished(0x10, 3, true, 42);
        assert_eq!(data, [0x10, 0, 0, 0, 0]);
    }

    #[test]
    fn finished_from_an_older_peer_is_read_as_success() {
        // v2-v4 carry no accepted bit; the spec says assume the drop worked.
        let decoded = decode_finished([0x10, 0, 0, 0, 0], 3);
        assert!(
            decoded.accepted,
            "pre-v5 has no bit to clear, so it means success"
        );
    }

    #[test]
    fn finished_rejection_round_trips_on_v5() {
        let decoded = decode_finished(encode_finished(0x10, 5, false, 42), 5);
        assert!(!decoded.accepted);
        assert_eq!(decoded.action, 0);
    }

    // ---------- type selection ----------

    #[test]
    fn type_selection_follows_our_preference_not_theirs() {
        // The source lists types in arbitrary order; our ranking decides.
        assert_eq!(choose_type(&[30, 20, 10], &[10, 20, 30]), Some(10));
        assert_eq!(choose_type(&[30, 20], &[10, 20, 30]), Some(20));
    }

    #[test]
    fn type_selection_returns_none_without_overlap() {
        assert_eq!(choose_type(&[99], &[10, 20]), None);
        assert_eq!(choose_type(&[], &[10]), None);
    }

    // `text/uri-list` encoding and decoding are not duplicated here: they are
    // shared with every other backend via `ExternalDropData::from_uri_list` /
    // `OutboundDragData::to_uri_list` in bastyde-core, and tested there.

    // ---------- INCR ----------

    #[test]
    fn incr_assembles_chunks_until_the_empty_terminator() {
        let mut incr = IncrAssembler::new(6);
        assert!(!incr.push(b"abc"));
        assert!(!incr.push(b"def"));
        assert!(incr.push(b""), "an empty chunk terminates the transfer");
        assert!(incr.is_complete());
        assert_eq!(incr.finish(), b"abcdef".to_vec());
    }

    #[test]
    fn incr_handles_an_immediately_empty_payload() {
        let mut incr = IncrAssembler::new(0);
        assert!(incr.push(b""));
        assert_eq!(incr.finish(), Vec::<u8>::new());
    }

    #[test]
    fn incr_ignores_a_wild_size_hint() {
        // A bogus hint must not translate into a huge up-front allocation.
        let mut incr = IncrAssembler::new(usize::MAX);
        incr.push(b"x");
        incr.push(b"");
        assert_eq!(incr.finish(), b"x".to_vec());
    }
}
