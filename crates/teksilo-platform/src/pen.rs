// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Pen and stylus input, ahead of winit.
//!
//! winit 0.30 exposes **no pen API at all**. On Windows its `WM_POINTER` arm
//! already decodes pen packets and hands the app nothing; on Wayland
//! `zwp_tablet_v2` is simply never bound. A pen therefore reaches a winit 0.30
//! client either as a mouse (Windows, via the OS's own promotion) or as
//! nothing (Wayland). Neither carries pressure, tilt, twist, the eraser end, or
//! the fact that the tool is *hovering*.
//!
//! This module is the shim that fills that gap without waiting for the winit
//! 0.31 upgrade, behind one seam:
//!
//! ```text
//!   OS                    PenSource::poll        TranslationState
//!   zwp_tablet_tool_v2 ─┐
//!   WM_POINTER* ────────┼─▶  Vec<PenPacket>  ─▶  Vec<PointerSample>
//!   (nothing) ──────────┘                        PointerKind::Pen(tool)
//! ```
//!
//! [`PenSource`] is *pulled*, not pushed: both backends buffer packets off the
//! event path (a Wayland dispatch thread, a Win32 subclass proc) and the caller
//! drains them once per event-loop turn through
//! [`TranslationState::poll_pen`](crate::event_translation::TranslationState::poll_pen).
//! That keeps the OS callbacks free of Teksilo state and gives the translator
//! the caller's clock, as the one-clock rule requires.
//!
//! # Support matrix
//!
//! | | pen at all | tool kind | pressure | tilt | twist | contact patch |
//! | --- | --- | --- | --- | --- | --- | --- |
//! | Wayland | [`wayland`] | yes | yes | yes | yes | — |
//! | Windows | [`windows`] | yes | yes | yes | yes | yes (touch) |
//! | X11 | [`null`] | no | no | no | no | no |
//! | macOS | [`null`] | no | no | no | no | no |
//!
//! X11 and macOS report their absence through [`BackendCaps`] rather than
//! pretending: `reports_pen_kind` and friends stay `false`, and a consumer that
//! must know asks instead of guessing from `cfg!(target_os = ...)`.
//!
//! # A drained batch has its own timeline
//!
//! A poll drains everything buffered since the last one, and those packets did
//! **not** all happen at the instant the poll ran. Each carries the device's
//! own millisecond counter in [`PenPacket::device_time_ms`]; [`back_date`]
//! turns that batch into one [`EventTime`] per packet — newest at the poll's
//! `now`, earlier ones at the device's own deltas before it — so a stroke's
//! velocity, its smoothing and its per-sample time offsets are computed from
//! when the digitizer says the samples happened rather than from when the
//! event loop got round to asking. A batch of one is stamped `now` exactly.
//!
//! The device counters themselves never escape the platform layer: their epoch
//! is unknown, so only their *differences* are ever read.
//!
//! # Proximity is a first-class state
//!
//! A pen in proximity with no contact is a **hovering pointer**: it moves,
//! drives hover visuals, tooltips and the cursor exactly as a mouse does, and
//! it does so with `down: false`. That is why [`PointerKind::hovers`] is true
//! for `Pen` and false for `Touch`. The proximity → contact → proximity-out
//! machine lives in the translator (`event_translation.rs`); a source's job is
//! only to report `in_proximity` and `down` truthfully per packet.
//!
//! # Buttons, normatively
//!
//! - Pen **contact** is [`PointerButton::Primary`](teksilo_core::event::PointerButton::Primary).
//! - The **barrel** button is
//!   [`Secondary`](teksilo_core::event::PointerButton::Secondary), matching
//!   W3C Pointer Events (pen barrel → `button` 2, `buttons` bit 2).
//! - A second barrel button, where the hardware has one, is
//!   [`Middle`](teksilo_core::event::PointerButton::Middle).
//! - The **eraser is a tool kind** ([`PenKind::Eraser`]), never a button. A
//!   digitizer that reports the eraser as a flag has that flag folded into the
//!   tool before it leaves the source; [`PenButtons::ERASER`] exists only so a
//!   backend can carry the raw bit faithfully.
//!
//! # This module has an expiry date
//!
//! winit 0.31 supersedes both shims with its own `TabletTool*` events and
//! `PointerSource::Tablet`. At that upgrade [`wayland`] and [`windows`] are
//! **deleted**, `create_pen_source` returns [`null::NullPenSource`] everywhere,
//! and the translator reads the pen off winit like every other device. Nothing
//! above this seam changes.
//!
//! Reference: `docs/touch-and-pen.md`, "Pen and stylus".
//!
//! [`BackendCaps`]: crate::pointer_backend::BackendCaps
//! [`PointerKind::hovers`]: teksilo_tokens::PointerKind::hovers

// The support matrix above links `wayland`, which is `#[cfg]`-ed away off Unix
// — so on Windows and macOS that link cannot resolve and a local
// `RUSTDOCFLAGS="-D warnings"` run fails on a link that is correct by
// construction. The docs that ship are built on Linux (both `ci.yml`'s doc gate
// and `docs.yml` are `runs-on: ubuntu-latest`), where the module exists, the
// link resolves, and every link in this module is still checked under
// `-D warnings`. Scoped to the hosts that cannot have the module, so a
// genuinely broken link here still fails the gate on the host that enforces it.
#![cfg_attr(
    not(all(unix, not(target_os = "macos"))),
    allow(rustdoc::broken_intra_doc_links)
)]

pub mod null;
#[cfg(all(unix, not(target_os = "macos")))]
pub mod wayland;
// Compiled on **every** target on purpose: the `POINTER_PEN_INFO` /
// `POINTER_TOUCH_INFO` decoder inside is pure byte-slice arithmetic, and it is
// tested from recorded layouts on hosts that have no Windows. Only the
// subclass shim that feeds it is `#[cfg(target_os = "windows")]`.
pub mod windows;

use teksilo_canvas::{Point, Size};
use teksilo_core::pointer::EventTime;
use teksilo_core::raw_handle::ParentHandle;
use teksilo_tokens::PenKind;

use crate::pointer_backend::BackendCaps;

// ---------------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------------

/// The stylus buttons a packet reports as held.
///
/// A bitset rather than a `Vec` because the set is small, fixed and copied on
/// every packet. See the module docs for the normative mapping onto
/// [`PointerButton`](teksilo_core::event::PointerButton) — in particular, the
/// tip is **not** in here (it is `down`), and [`ERASER`](Self::ERASER) is a
/// carried flag rather than a button Teksilo dispatches.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct PenButtons(u8);

impl PenButtons {
    /// No stylus button held.
    pub const NONE: Self = Self(0);
    /// The barrel button — the one every stylus has. Dispatched as
    /// `PointerButton::Secondary`.
    pub const BARREL: Self = Self(1 << 0);
    /// A second barrel button, where the hardware has one (Wayland's
    /// `BTN_STYLUS2`). Dispatched as `PointerButton::Middle`.
    pub const SECONDARY_BARREL: Self = Self(1 << 1);
    /// The digitizer's "eraser" flag. Carried for fidelity and folded into
    /// [`PenKind::Eraser`] by the source; never dispatched as a button.
    pub const ERASER: Self = Self(1 << 2);

    /// The raw bits, for a backend that must store the set compactly.
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Whether every button in `other` is held.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Nothing held.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The union of two sets.
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// `other` removed from this set.
    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// `other` added to or removed from this set.
    pub const fn with(self, other: Self, held: bool) -> Self {
        if held {
            self.union(other)
        } else {
            self.without(other)
        }
    }
}

// ---------------------------------------------------------------------------
// Capabilities
// ---------------------------------------------------------------------------

/// What a [`PenSource`] can actually report.
///
/// Folded into the window's [`BackendCaps`] by
/// [`apply_to`](Self::apply_to), so a consumer keeps asking one question
/// ("does this window report tilt?") whether the answer comes from winit or
/// from a shim.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct PenCaps {
    /// The source distinguishes a pen tip from an eraser (and the other
    /// [`PenKind`]s).
    pub tool_kind: bool,
    /// Tip pressure is reported.
    pub pressure: bool,
    /// Tilt is reported.
    pub tilt: bool,
    /// Barrel rotation is reported.
    pub twist: bool,
    /// The source reports a touch contact patch (Windows `rcContact`).
    pub touch_contact: bool,
}

impl PenCaps {
    /// A source that reports nothing — the honest answer on X11 and macOS.
    pub const NONE: Self = Self {
        tool_kind: false,
        pressure: false,
        tilt: false,
        twist: false,
        touch_contact: false,
    };

    /// Everything a full digitizer reports, minus the contact patch.
    pub const FULL_PEN: Self = Self {
        tool_kind: true,
        pressure: true,
        tilt: true,
        twist: true,
        touch_contact: false,
    };

    /// Raise the matching flags on a window's platform capabilities.
    ///
    /// Only ever *raises* them: a shim adds a capability winit lacks, it never
    /// takes one away.
    pub fn apply_to(self, caps: &mut BackendCaps) {
        caps.reports_pen_kind |= self.tool_kind;
        caps.reports_pressure |= self.pressure;
        caps.reports_tilt |= self.tilt;
        caps.reports_twist |= self.twist;
    }
}

// ---------------------------------------------------------------------------
// Packets
// ---------------------------------------------------------------------------

/// One digitizer packet, normalised.
///
/// A packet is a *level*, not an edge: it describes the tool's complete state
/// at one instant, and the translator derives the transitions (enter, down, up,
/// leave, button changes) by comparing consecutive packets. That is the shape
/// both backends produce naturally — Wayland accumulates axes and commits them
/// on `frame`, Win32 fills one `POINTER_PEN_INFO` per message — and it means a
/// dropped packet costs a sample, never a stuck button.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct PenPacket {
    /// The tool the digitizer says is in use. The eraser end is a tool, not a
    /// button.
    pub tool: PenKind,
    /// Window-logical position, already divided by the window's scale factor.
    pub position: Point,
    /// Normalised tip pressure, `0.0..=1.0`. `0.0` while hovering.
    pub pressure: f32,
    /// `(tilt_x, tilt_y)` in degrees, each `-90.0..=90.0`, or `None` when the
    /// tool has no tilt axis.
    pub tilt: Option<(f32, f32)>,
    /// Barrel rotation in degrees, `0.0..=359.0`, or `None` when the tool has
    /// no rotation axis.
    pub twist: Option<f32>,
    /// The stylus buttons held.
    pub buttons: PenButtons,
    /// Whether the tool is within the digitizer's detection range. A packet
    /// with `in_proximity: false` ends the hover session.
    pub in_proximity: bool,
    /// Whether the tip is touching the surface.
    pub down: bool,
    /// The device's own millisecond counter, on whatever clock the OS uses,
    /// or `None` from a source that has no clock at all.
    ///
    /// Deliberately **not** an [`EventTime`]: the epoch is unknown. Wayland's
    /// `frame` time is the compositor's, Win32's `dwTime` is
    /// `GetTickCount`'s, and neither has a known offset from the tree's epoch,
    /// so as an absolute this number is a lie dressed as precision.
    ///
    /// **Within one drained batch it is exact as a relative**, and that is the
    /// only way it is ever read: [`back_date`] places a batch on the tree's
    /// timeline by anchoring the newest packet at the poll's `now` and walking
    /// backwards through these deltas. That is what stops twenty packets
    /// spanning one drain from all claiming a single instant, which is what
    /// they did while this field did not exist.
    ///
    /// A `u32` because both platforms report one, wrap included: `back_date`
    /// reads the deltas with `wrapping_sub`, so a counter rolling over inside
    /// a batch costs nothing.
    pub device_time_ms: Option<u32>,
}

impl PenPacket {
    /// A hovering packet: in proximity, tip up, no pressure, no buttons.
    pub fn hovering(tool: PenKind, position: Point) -> Self {
        Self {
            tool,
            position,
            pressure: 0.0,
            tilt: None,
            twist: None,
            buttons: PenButtons::NONE,
            in_proximity: true,
            down: false,
            device_time_ms: None,
        }
    }

    /// The packet a tool leaving the digitizer's range produces. Position is
    /// the last known one — the tool did not move, it stopped being seen.
    pub fn out_of_proximity(tool: PenKind, position: Point) -> Self {
        Self {
            in_proximity: false,
            ..Self::hovering(tool, position)
        }
    }

    /// This packet with the tip in contact at `pressure`.
    pub fn down_at(mut self, pressure: f32) -> Self {
        self.down = true;
        self.pressure = pressure.clamp(0.0, 1.0);
        self
    }

    /// This packet stamped with the device's own millisecond counter.
    ///
    /// The value a shim reads off the digitizer — Wayland's `frame` time,
    /// Win32's `dwTime`. See [`device_time_ms`](Self::device_time_ms) for why
    /// it stays a raw counter rather than becoming an [`EventTime`].
    pub const fn at_device_ms(mut self, ms: u32) -> Self {
        self.device_time_ms = Some(ms);
        self
    }
}

// ---------------------------------------------------------------------------
// Placing a drained batch on the tree's timeline
// ---------------------------------------------------------------------------

/// A device delta longer than this is read as the counter having run
/// *backwards* — an out-of-order stamp, or a counter reset — rather than as a
/// real pause inside one drained batch.
///
/// The number only has to separate a plausible forward delta from the ~4.29
/// billion a `wrapping_sub` yields when `cur < prev`, so it is set generously.
/// A drained batch spans one [`PEN_POLL_INTERVAL`] in the steady state; ten
/// seconds inside one drain is already nonsense.
pub const MAX_DEVICE_GAP_MS: u64 = 10_000;

/// Place a drained batch of packets on the tree's timeline, newest at `now`.
///
/// `device_ms` is each packet's [`PenPacket::device_time_ms`], **oldest
/// first**, exactly as [`PenSource::poll`] appends them. The result is the
/// same length and the same order.
///
/// The rule:
///
/// - The newest packet is `now`. It is the one the poll's clock actually
///   describes, and a batch of one is therefore stamped `now` exactly — which
///   is what every packet was stamped before this function existed.
/// - Each earlier packet sits at the device's own delta before the one after
///   it, read with `wrapping_sub` so a `u32` counter rolling over inside the
///   batch costs nothing. A delta past [`MAX_DEVICE_GAP_MS`] is the counter
///   running backwards and falls through to the step below.
/// - Where either neighbour reports no device clock, the step is one
///   [`PEN_POLL_INTERVAL`] divided evenly across the batch, so the whole batch
///   still fits inside the window it was buffered in.
/// - Times saturate at [`EventTime::ZERO`] and never exceed `now`.
///
/// Monotone non-decreasing by construction, and **strictly** increasing
/// whenever the device's own stamps strictly increase. Two packets the
/// digitizer stamped in the same millisecond stay equal: that is what the
/// device said, and inventing a gap would be inventing precision.
///
/// Pure, so the rule is testable with no digitizer:
///
/// ```
/// use teksilo_platform::pen::back_date;
/// use teksilo_core::pointer::EventTime;
///
/// let now = EventTime::from_millis(1_000);
/// assert_eq!(
///     back_date(now, &[Some(40), Some(44), Some(52)]),
///     vec![
///         EventTime::from_millis(988),
///         EventTime::from_millis(992),
///         now,
///     ],
/// );
/// // A batch of one is `now`.
/// assert_eq!(back_date(now, &[Some(7)]), vec![now]);
/// ```
pub fn back_date(now: EventTime, device_ms: &[Option<u32>]) -> Vec<EventTime> {
    let mut out = Vec::with_capacity(device_ms.len());
    back_date_into(now, device_ms, &mut out);
    out
}

/// [`back_date`] into a caller-owned buffer, so the pen pump allocates nothing
/// per drain.
///
/// `out` is cleared first and left the same length as `device_ms`.
pub fn back_date_into(now: EventTime, device_ms: &[Option<u32>], out: &mut Vec<EventTime>) {
    out.clear();
    let count = device_ms.len();
    if count == 0 {
        return;
    }
    // One poll interval divided evenly is the step wherever the device says
    // nothing — `count`, not `count - 1`, so even the oldest packet stays
    // strictly inside the interval it was buffered in.
    let step = PEN_POLL_INTERVAL / count as u32;

    // Pass one: a relative timeline, `EventTime` standing in for a `Duration`
    // since the batch's own start so no second buffer is needed.
    let mut elapsed = std::time::Duration::ZERO;
    out.push(EventTime::from_duration(elapsed));
    for index in 1..count {
        let advance = match (device_ms[index - 1], device_ms[index]) {
            (Some(previous), Some(current)) => {
                let delta = u64::from(current.wrapping_sub(previous));
                if delta <= MAX_DEVICE_GAP_MS {
                    std::time::Duration::from_millis(delta)
                } else {
                    step
                }
            }
            _ => step,
        };
        elapsed = elapsed.saturating_add(advance);
        out.push(EventTime::from_duration(elapsed));
    }

    // Pass two: slide the timeline so its newest entry lands on `now`.
    let span = elapsed;
    for slot in out.iter_mut() {
        // `span >= slot` always: the timeline is non-decreasing and `span` is
        // its last entry.
        let before_now = span - slot.as_duration();
        *slot = EventTime::from_duration(now.as_duration().saturating_sub(before_now));
    }
}

// ---------------------------------------------------------------------------
// The source
// ---------------------------------------------------------------------------

/// How a drained pen batch reaches the tree.
///
/// A digitizer runs at 200-360 Hz and a window's message rate does not, so one
/// `poll_pen` drain routinely holds several packets. Both answers to "what does
/// the tree see?" are defensible and the trade is real, which is why this is a
/// knob rather than a decision baked in:
///
/// - [`PerPacket`](Self::PerPacket) spends a whole tree dispatch — hit test,
///   arbitration turn, handler walk — on every packet. Nothing is lost and
///   nothing is coalesced; it is what the pen path has always done.
/// - [`Coalesce`](Self::Coalesce) spends one dispatch per drain and hands the
///   intermediate positions over as
///   [`PointerSample::coalesced`](teksilo_core::PointerSample::coalesced),
///   each keeping its own time and its own axes. A surface that reads
///   `EventContext::coalesced` sees exactly the same positions; one that does
///   not sees fewer moves.
///
/// **Transitions are never folded.** Down, Up, a button change and proximity
/// enter/leave each keep their own sample under either mode, so no recognizer
/// sees a different *sequence* — only the number of `PointerMove`s between two
/// transitions changes.
///
/// Four of those five the fold can *see* for itself, because all it tests is
/// the phase and the button: Down and Up are not `Move`, a button change reports
/// a button, and proximity **leave** is a
/// [`PointerPhase::Cancel`](teksilo_core::PointerPhase::Cancel), so it breaks a
/// run for free. The fifth it cannot, and that one is handled by name rather
/// than by luck: proximity **enter** has no phase of its own — it is carried as
/// a move with nothing held — so `poll_pen` pins it, because folding it away
/// would not cost a `PointerMove` but a `PointerEnter`: the tree derives the
/// hover owner, the cursor and the tooltip dwell from a sample's position and
/// never from its batched list, and a tool that came into range over one widget
/// and hovered onto another inside one drain would otherwise never enter the
/// first.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
#[non_exhaustive]
pub enum PenBatching {
    /// One [`PointerSample`](teksilo_core::PointerSample) per packet. The
    /// default, and what the pen path did before this existed.
    #[default]
    PerPacket,
    /// One sample per *transition*; the pure-motion packets between two
    /// transitions ride in
    /// [`PointerSample::coalesced`](teksilo_core::PointerSample::coalesced).
    Coalesce,
}

/// A buffered supply of [`PenPacket`]s.
///
/// One instance per window. Implementations are expected to be cheap to poll
/// and to return promptly with nothing when the user is not holding a stylus,
/// because the caller polls once per event-loop turn.
///
/// `Debug` is a supertrait so that a `TranslationState` holding one stays
/// `Debug` — the whole per-window translator is dumped in traces and in the
/// inspector.
pub trait PenSource: std::fmt::Debug {
    /// Append every packet buffered since the last poll to `out`, oldest
    /// first, and clear the buffer.
    ///
    /// Appending rather than returning a `Vec` lets the caller reuse one
    /// scratch buffer for the life of the window.
    ///
    /// **Oldest first is load-bearing**, not a convenience: the caller reads
    /// the run as one timeline and back-dates it from the last entry (see
    /// [`back_date`]). A source whose OS hands it the newest entry first —
    /// Win32's `GetPointerPenInfoHistory` does exactly that — reverses before
    /// appending.
    ///
    /// Each packet should carry the device's own stamp in
    /// [`PenPacket::device_time_ms`] where the platform reports one. A source
    /// that leaves it `None` is not wrong; its batch is simply spread evenly
    /// over one [`PEN_POLL_INTERVAL`] instead of by the device's deltas.
    fn poll(&mut self, out: &mut Vec<PenPacket>);

    /// What this source reports. Defaults to [`PenCaps::NONE`], which is the
    /// correct answer for a source that yields no packets.
    fn capabilities(&self) -> PenCaps {
        PenCaps::NONE
    }

    /// Whether this source fills its buffer from a thread of its own.
    ///
    /// The event loop needs to know, because it decides whether draining once
    /// per turn is enough. A shim that reads on the **winit thread** — the
    /// Windows `WM_POINTER` subclass — has already filled its buffer by the
    /// time the turn that carried the message reaches the pump, so one drain
    /// per turn sees everything. A shim that reads on its **own** thread — the
    /// Wayland tablet listener — has not: the compositor event that woke the
    /// loop and the packet the shim will make of it are up to one of that
    /// listener's dispatch intervals apart, so the loop has to look again. The
    /// catch-up look is armed at [`PEN_POLL_INTERVAL`], which is the interval
    /// that applies once a tool has been announced; a session with none is on
    /// a slower tier, and cannot deliver a packet at all until `tool_added`
    /// has moved it to the fast one.
    ///
    /// Defaults to `false`, which is the answer for a source with no thread.
    fn polls_off_thread(&self) -> bool {
        false
    }

    /// The contact patch most recently reported for an OS touch contact id, in
    /// logical pixels.
    ///
    /// This is not pen data, and it is here for one reason: on Windows the
    /// `WM_POINTER` family carries `POINTER_TOUCH_INFO::rcContact`, which
    /// `WM_TOUCH` — the path winit 0.30 takes — does not. The same subclass
    /// that reads pen packets can read it, and the palm heuristic and the
    /// finger-avoiding overlay placement both want it. Every other source
    /// returns `None`.
    ///
    /// Keyed by the raw OS contact id (Windows' `pointerId`), because that is
    /// what winit puts in `Touch::id` on the `WM_POINTER` path.
    fn touch_contact(&self, _os_contact_id: u64) -> Option<Size> {
        None
    }
}

/// The pen source for a window, or [`null::NullPenSource`] where the platform
/// has none.
///
/// Never fails: a window whose tablet manager is missing, whose subclass would
/// not install, or which runs on a platform with no pen path at all, gets the
/// null source and reports its absence through [`PenCaps::NONE`].
pub fn create_pen_source(parent: &ParentHandle) -> Box<dyn PenSource> {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(source) = wayland::WaylandPenSource::attach(parent) {
            return Box::new(source);
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(source) = windows::WindowsPenSource::attach(parent) {
            return Box::new(source);
        }
    }
    let _ = parent;
    Box::new(null::NullPenSource::new())
}

/// How often a pen shim that reads on its own thread looks at the digitizer.
///
/// The Wayland shim's own sleep interval, published so the event loop can pace
/// its catch-up look to it rather than guessing. The bound that makes a value
/// wrong is the velocity tracker's
/// [`STOP_GAP`](teksilo_core::kinetic::velocity::STOP_GAP): a gap that long
/// between samples is read as the stroke having paused and clears the history,
/// so a shim looking that rarely would turn one continuous stroke into a
/// sequence of standing starts. That relation is asserted in this module's
/// tests rather than left to this sentence.
pub const PEN_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(4);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pointer_backend::{PlatformKind, PointerBackend};
    use crate::window_system::WindowSystem;

    #[test]
    fn buttons_are_a_set() {
        let held = PenButtons::NONE.with(PenButtons::BARREL, true);
        assert!(held.contains(PenButtons::BARREL));
        assert!(!held.contains(PenButtons::SECONDARY_BARREL));
        assert!(!held.is_empty());
        assert!(held.without(PenButtons::BARREL).is_empty());
        // Removing something that was never held is not an error.
        assert_eq!(held.without(PenButtons::ERASER), held);
    }

    #[test]
    fn caps_only_raise_never_lower() {
        // macOS reports no pressure; a source that does must not be able to
        // *unset* a capability winit already claimed either.
        let mut caps = BackendCaps::for_platform(PlatformKind::Windows, WindowSystem::Unknown);
        assert!(caps.reports_pressure);
        PenCaps::NONE.apply_to(&mut caps);
        assert!(caps.reports_pressure, "NONE must not clear a set flag");
        assert!(!caps.reports_tilt);
        PenCaps::FULL_PEN.apply_to(&mut caps);
        assert!(caps.reports_tilt && caps.reports_twist && caps.reports_pen_kind);
    }

    /// The one property that makes [`PEN_POLL_INTERVAL`] right or wrong.
    ///
    /// A shim reading on its own thread hands the translator samples no fresher
    /// than one interval. If that interval reached the velocity tracker's
    /// `STOP_GAP`, every sample would look to the tracker like the resumption
    /// of a stroke that had stopped, and a pen fling would be estimated from
    /// standing starts. Well under it is the requirement; the exact figure is
    /// the Wayland shim's sleep.
    #[test]
    fn the_poll_interval_stays_under_the_velocity_stop_gap() {
        use teksilo_core::kinetic::velocity::STOP_GAP;
        assert!(
            PEN_POLL_INTERVAL < STOP_GAP,
            "a poll interval at or past the {STOP_GAP:?} stop gap clears the \
             velocity history between samples"
        );
    }

    // -----------------------------------------------------------------------
    // back_date
    // -----------------------------------------------------------------------

    /// The invariant the old behaviour got right and a back-dating design must
    /// not break: one packet is the poll's clock, exactly.
    #[test]
    fn a_batch_of_one_is_the_polls_own_clock() {
        let now = EventTime::from_millis(1234);
        assert_eq!(back_date(now, &[Some(99_000)]), vec![now]);
        assert_eq!(back_date(now, &[None]), vec![now]);
        assert!(back_date(now, &[]).is_empty());
    }

    /// The defect this exists to kill: every packet in a drain claiming one
    /// instant. With the device's own stamps the batch is strictly increasing
    /// and carries the digitizer's spacing, not the poll's.
    #[test]
    fn a_batch_keeps_the_devices_own_spacing() {
        let now = EventTime::from_millis(1_000);
        let times = back_date(now, &[Some(40), Some(44), Some(52), Some(53)]);
        assert_eq!(
            times,
            vec![
                EventTime::from_millis(987),
                EventTime::from_millis(991),
                EventTime::from_millis(999),
                now,
            ]
        );
        for pair in times.windows(2) {
            assert!(pair[1] > pair[0], "{times:?} must strictly increase");
        }
    }

    /// A device whose counter rolls over mid-batch is still a forward stroke.
    /// `wrapping_sub` is what makes the `u32` safe to subtract.
    #[test]
    fn a_wrapping_counter_is_still_a_forward_delta() {
        let now = EventTime::from_millis(500);
        let times = back_date(now, &[Some(u32::MAX - 3), Some(u32::MAX), Some(4)]);
        assert_eq!(
            times,
            vec![
                EventTime::from_millis(492),
                EventTime::from_millis(495),
                now,
            ],
            "MAX-3 → MAX is 3 ms and MAX → 4 is 5 ms across the wrap"
        );
    }

    /// A stamp that goes *backwards* is not a 49-day pause. It falls through
    /// to the even step rather than back-dating the batch into the last
    /// century.
    #[test]
    fn a_backwards_stamp_falls_back_to_the_even_step() {
        let now = EventTime::from_millis(100);
        let times = back_date(now, &[Some(900), Some(100)]);
        let step = PEN_POLL_INTERVAL / 2;
        assert_eq!(
            times,
            vec![EventTime::from_duration(now.as_duration() - step), now]
        );
    }

    /// No device clock at all: the batch is still spread, because the packets
    /// are separate digitizer frames and calling them simultaneous is the
    /// original bug in miniature.
    #[test]
    fn a_batch_with_no_device_clock_divides_the_poll_interval() {
        let now = EventTime::from_millis(100);
        let times = back_date(now, &[None, None, None]);
        let step = PEN_POLL_INTERVAL / 3;
        assert_eq!(
            times,
            vec![
                EventTime::from_duration(now.as_duration() - step * 2),
                EventTime::from_duration(now.as_duration() - step),
                now,
            ]
        );
        // The whole batch stays inside the window it was buffered in.
        assert!(now.saturating_since(times[0]) < PEN_POLL_INTERVAL);
    }

    /// A source that stamps some packets and not others is pathological, not
    /// impossible. It must still come out ordered.
    #[test]
    fn a_partly_stamped_batch_stays_monotone() {
        let now = EventTime::from_millis(1_000);
        let times = back_date(now, &[Some(10), None, Some(30), Some(31)]);
        assert_eq!(times.len(), 4);
        for pair in times.windows(2) {
            assert!(pair[0] <= pair[1], "{times:?} must not go backwards");
        }
        assert_eq!(*times.last().unwrap(), now);
    }

    /// Equal device stamps stay equal. Two packets the digitizer stamped in
    /// the same millisecond really were in the same millisecond, and
    /// manufacturing a gap would be manufacturing precision.
    #[test]
    fn equal_device_stamps_stay_equal() {
        let now = EventTime::from_millis(50);
        assert_eq!(back_date(now, &[Some(7), Some(7)]), vec![now, now]);
    }

    /// Nothing is ever placed in the future, and nothing underflows the epoch.
    #[test]
    fn the_batch_is_clamped_to_the_epoch_and_to_now() {
        let now = EventTime::from_millis(2);
        let times = back_date(now, &[Some(0), Some(500), Some(1_000)]);
        assert_eq!(times, vec![EventTime::ZERO, EventTime::ZERO, now]);
        assert!(times.iter().all(|&t| t <= now));
    }

    /// `back_date_into` is the allocation-free twin, and answers identically.
    #[test]
    fn the_into_form_agrees_with_the_allocating_one() {
        let now = EventTime::from_millis(777);
        let stamps = [Some(1), Some(3), None, Some(9)];
        let mut buffer = vec![EventTime::from_millis(42); 9];
        back_date_into(now, &stamps, &mut buffer);
        assert_eq!(buffer, back_date(now, &stamps));
        back_date_into(now, &[], &mut buffer);
        assert!(buffer.is_empty(), "an empty drain clears the buffer");
    }

    #[test]
    fn the_null_source_reports_no_pen_and_yields_nothing() {
        let mut source = null::NullPenSource::new();
        let mut out = Vec::new();
        source.poll(&mut out);
        assert!(out.is_empty(), "the null source must yield no packets");
        assert_eq!(source.capabilities(), PenCaps::NONE);
        assert_eq!(source.touch_contact(1), None);

        // And a translator carrying it advertises no pen either.
        let mut state = crate::event_translation::TranslationState::new();
        state.set_pen_source(Box::new(null::NullPenSource::new()));
        let caps = state.capabilities();
        assert!(!caps.reports_pen_kind);
        assert!(!caps.reports_tilt);
        assert!(!caps.reports_twist);
        assert!(
            state.poll_pen(EventTime::from_millis(10)).is_empty(),
            "no packets in, no samples out"
        );
    }
}
