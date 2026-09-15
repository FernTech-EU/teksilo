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
    /// The backend's timestamp **on the tree's timeline**, or
    /// [`EventTime::ZERO`] when it has none.
    ///
    /// Both shipped shims stamp `ZERO`: Wayland's `frame` time and Win32's
    /// `dwTime` are millisecond counters on device clocks with no known offset
    /// from the tree's epoch, and inventing one would be a lie dressed as
    /// precision. The translator then stamps the poll's `now`, which is the
    /// tree's own clock and is never more than one event-loop turn late.
    pub time: EventTime,
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
            time: EventTime::ZERO,
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
}

// ---------------------------------------------------------------------------
// The source
// ---------------------------------------------------------------------------

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
