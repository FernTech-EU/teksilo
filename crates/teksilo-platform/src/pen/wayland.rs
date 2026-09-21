// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Wayland tablet support (`zwp_tablet_v2`).
//!
//! winit 0.30 never binds `zwp_tablet_manager_v2`, so under Wayland a stylus is
//! not "a mouse without pressure" — it is **nothing at all**. This module binds
//! the protocol on winit's own `wl_display` and turns tool events into
//! [`PenPacket`]s.
//!
//! # The connection, and the one rule about it
//!
//! Exactly the model [`external_dnd::wayland`](crate::external_dnd) proved:
//! wrap winit's live `wl_display` with `Backend::from_foreign_display`, run
//! `registry_queue_init` on it, bind the globals we need, and drain **our**
//! queue from a dedicated thread with `dispatch_pending`.
//!
//! The rule, restated because breaking it aborts the process: **never read the
//! socket**. winit's event loop is its sole reader; a second reader
//! (`blocking_dispatch` → `prepare_read` / `read_events`) is a fatal error in
//! libwayland. The multi-queue model buffers events for our objects whenever
//! *anyone* reads, so `dispatch_pending` on a short interval is both correct
//! and sufficient. While a tool is live the interval here is shorter than the
//! drag backend's, because a stylus is a continuous input where a drag is a
//! discrete one; while no tool has been announced it is far longer, because
//! binding the protocol says nothing about whether a digitizer exists — see
//! `WaylandPenSource::poll_interval`.
//!
//! # Surface-local is already logical
//!
//! Tablet motion arrives in **surface-local** coordinates, which on Wayland are
//! logical: the compositor has already divided by the buffer scale. So unlike
//! the Windows arm — which reads screen *physical* pixels and divides — this
//! one passes positions through untouched. winit does the same thing in the
//! other direction (it *multiplies* surface-local by the scale factor to
//! report physical), which is why the two look inconsistent and are not.
//!
//! # Tools that are not pens
//!
//! `zwp_tablet_tool_v2::Type` names eight tools; two of them are not styluses.
//!
//! - `Finger` is a finger on the tablet surface. It is a touch contact, it is
//!   already delivered as one through `wl_touch`, and calling it
//!   `PointerKind::Pen` would give it a stylus's tuning — tight slop, no hit
//!   outset, precise-pointer affordances — which is exactly wrong for a
//!   fingertip. **Dropped here.**
//! - `Mouse` is a puck-style tablet mouse: an indirect device with no tip
//!   pressure, whose events do not reach `wl_pointer`. `PenKind` has no
//!   variant for it, and forcing one would misreport an indirect pointer as a
//!   direct one. **Dropped here**, and named as a gap in
//!   `docs/touch-and-pen.md` rather than papered over.
//!
//! `Lens` — the other puck — *is* mapped, to [`PenKind::Lens`]: it is an
//! absolute-positioning tool on the tablet surface, which is what "direct"
//! means here.
//!
//! # Testing without a compositor
//!
//! All the state — the axis accumulator, the proximity machine, the frame
//! commit — lives in [`ToolState`], which knows nothing about wayland-client
//! and is driven by recorded event sequences in this module's tests.
//!
//! `translate_tool_event` is tested one layer below that, against real
//! `zwp_tablet_tool_v2::Event` values. It is worth its own tests because it is
//! **not** a variant rename: `frame` carries a millisecond stamp, `motion` a
//! coordinate pair, `button` a code and a state, so a decode that drops a field
//! compiles and runs and is wrong — which is how `frame`'s clock went missing
//! once already. A protocol `Event` is plain data, so every arm that reads a
//! field can be pushed through the decode on a host with no compositor.
//!
//! The exception is `proximity_in`: it carries live `zwp_tablet_v2` and
//! `wl_surface` proxies, and a `Proxy` needs a connection. So does the `Dispatch`
//! glue, which transforms nothing — it looks the tool up by `ObjectId`, calls
//! [`ToolState::apply`], and forgets a removed tool.
//!
//! Reference: `docs/touch-and-pen.md`, "Pen and stylus".

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use teksilo_canvas::Point;
use teksilo_core::raw_handle::ParentHandle;
use teksilo_core::trace_input;
use teksilo_tokens::PenKind;

use wayland_backend::client::ObjectId;
use wayland_backend::sys::client::Backend;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry::WlRegistry;
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum, event_created_child};
use wayland_protocols::wp::tablet::zv2::client::{
    zwp_tablet_manager_v2::ZwpTabletManagerV2,
    zwp_tablet_pad_group_v2::{self, ZwpTabletPadGroupV2},
    zwp_tablet_pad_ring_v2::ZwpTabletPadRingV2,
    zwp_tablet_pad_strip_v2::ZwpTabletPadStripV2,
    zwp_tablet_pad_v2::{self, ZwpTabletPadV2},
    zwp_tablet_seat_v2::{self, ZwpTabletSeatV2},
    zwp_tablet_tool_v2::{self, ZwpTabletToolV2},
    zwp_tablet_v2::ZwpTabletV2,
};

use super::{PenButtons, PenCaps, PenPacket, PenSource};

/// How often the dispatch thread drains its queue.
///
/// Half the drag backend's 8 ms: a stylus is a continuous input a user watches
/// ink follow, and 4 ms is a quarter of a 60 Hz frame — under the threshold at
/// which added latency is visible in a stroke, without spinning a thread.
use super::PEN_POLL_INTERVAL as POLL_INTERVAL;

/// `zwp_tablet_tool_v2::pressure` is normalised over this range.
const PRESSURE_RANGE: f32 = 65535.0;

/// `BTN_STYLUS` from Linux's `input-event-codes.h` — the barrel button.
const BTN_STYLUS: u32 = 0x14b;
/// `BTN_STYLUS2` — a second barrel button, where the hardware has one.
const BTN_STYLUS2: u32 = 0x14c;

// ---------------------------------------------------------------------------
// The pure state machine
// ---------------------------------------------------------------------------

/// One `zwp_tablet_tool_v2` event, stripped of wayland-client's types.
///
/// A one-to-one mirror of the protocol's event set (minus the axes Teksilo has
/// no home for — `distance`, `slider` and `wheel`), so a recorded sequence in
/// a test reads like the protocol dump it stands in for.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum ToolEvent {
    /// `type` — the raw `zwp_tablet_tool_v2::Type` value.
    Type(u32),
    /// `proximity_in`, carrying the surface's protocol id.
    ProximityIn { surface: u32 },
    /// `proximity_out`.
    ProximityOut,
    /// `down` — the tip touched the surface.
    Down,
    /// `up` — the tip left the surface.
    Up,
    /// `motion`, in surface-local (logical) coordinates.
    Motion { x: f64, y: f64 },
    /// `pressure`, `0..=65535`.
    Pressure(u32),
    /// `tilt`, in degrees.
    Tilt { x: f64, y: f64 },
    /// `rotation`, in degrees.
    Rotation(f64),
    /// `button`, with a Linux button code.
    Button { button: u32, pressed: bool },
    /// `frame` — commit everything accumulated since the last one.
    ///
    /// `time_ms` is the protocol's own stamp ("the time of the event with
    /// millisecond granularity", `tablet-v2.xml`). It is on the compositor's
    /// clock, so it reaches a [`PenPacket`] as
    /// [`device_time_ms`](PenPacket::device_time_ms) and is only ever read as
    /// a difference.
    Frame { time_ms: u32 },
    /// `removed` — the tool is gone. Treated as a proximity-out that cannot be
    /// followed by anything, so it commits its own leave packet rather than
    /// waiting for a `frame` that will never arrive.
    Removed,
}

/// Map `zwp_tablet_tool_v2::Type` onto a [`PenKind`].
///
/// `None` for the two tools that are not styluses; see the module docs.
pub fn tool_kind(raw: u32) -> Option<PenKind> {
    match raw {
        0x140 => Some(PenKind::Pen),
        0x141 => Some(PenKind::Eraser),
        0x142 => Some(PenKind::Brush),
        0x143 => Some(PenKind::Pencil),
        0x144 => Some(PenKind::Airbrush),
        // 0x145 Finger and 0x146 Mouse are deliberately not pens.
        0x147 => Some(PenKind::Lens),
        _ => None,
    }
}

/// Everything one tool accumulates between `frame`s.
///
/// The protocol is *event-per-axis, committed by `frame`*; a [`PenPacket`] is a
/// whole state. This is the adapter between the two, and it is the only place
/// the Wayland arm has any logic at all.
#[derive(Clone, Debug, Default)]
pub struct ToolState {
    /// The tool kind, once the `type` event has arrived. `None` means either
    /// "not announced yet" or "announced as something that is not a pen", and
    /// both mean the same thing here: emit nothing.
    tool: Option<PenKind>,
    /// Protocol id of the surface the current proximity session is over.
    surface: Option<u32>,
    in_proximity: bool,
    down: bool,
    position: Point,
    pressure: f32,
    tilt: Option<(f32, f32)>,
    twist: Option<f32>,
    buttons: PenButtons,
    /// Whether the last committed packet said `in_proximity`. Drives the one
    /// trailing packet a proximity-out has to produce so the hover ends.
    reported_proximity: bool,
    /// The stamp of the last `frame` seen, so the leave packet a `removed`
    /// synthesises can be dated at all. `None` until the first frame, which is
    /// a state no leave is ever built from: a tool that has not framed has not
    /// announced a hover either, so [`Self::commit`]'s
    /// `!in_proximity && !reported_proximity` guard turns its `removed` into no
    /// packet rather than into a packet with no time. See
    /// `a_tool_that_never_announced_a_hover_never_retracts_one`.
    last_frame_ms: Option<u32>,
    /// Whether anything changed since the last `frame`.
    dirty: bool,
}

impl ToolState {
    /// Apply one event, pushing a packet onto `out` when a `frame` commits one.
    ///
    /// `our_surface` is the protocol id of the window this source belongs to:
    /// a tablet seat is per-seat, not per-surface, so a tool hovering a sibling
    /// window arrives here too and must be ignored.
    pub fn apply(&mut self, event: ToolEvent, our_surface: u32, out: &mut Vec<PenPacket>) {
        match event {
            ToolEvent::Type(raw) => self.tool = tool_kind(raw),
            ToolEvent::ProximityIn { surface } => {
                self.surface = Some(surface);
                self.in_proximity = true;
                self.down = false;
                self.pressure = 0.0;
                self.buttons = PenButtons::NONE;
                self.dirty = true;
            }
            ToolEvent::ProximityOut => {
                self.in_proximity = false;
                self.down = false;
                self.pressure = 0.0;
                self.buttons = PenButtons::NONE;
                self.dirty = true;
            }
            ToolEvent::Removed => {
                self.in_proximity = false;
                self.down = false;
                self.pressure = 0.0;
                self.buttons = PenButtons::NONE;
                self.dirty = true;
                // A removed tool never frames again, so the leave is committed
                // here rather than waiting for a frame that will not come. It
                // carries the last frame's stamp, which is the most recent
                // time the compositor gave us for this tool.
                if let Some(packet) = self.commit(our_surface, self.last_frame_ms) {
                    out.push(packet);
                }
            }
            ToolEvent::Down => {
                self.down = true;
                self.dirty = true;
            }
            ToolEvent::Up => {
                self.down = false;
                self.pressure = 0.0;
                self.dirty = true;
            }
            ToolEvent::Motion { x, y } => {
                self.position = Point::new(x as f32, y as f32);
                self.dirty = true;
            }
            ToolEvent::Pressure(raw) => {
                self.pressure = (raw as f32 / PRESSURE_RANGE).clamp(0.0, 1.0);
                self.dirty = true;
            }
            ToolEvent::Tilt { x, y } => {
                self.tilt = Some(((x as f32).clamp(-90.0, 90.0), (y as f32).clamp(-90.0, 90.0)));
                self.dirty = true;
            }
            ToolEvent::Rotation(degrees) => {
                self.twist = Some(degrees.rem_euclid(360.0) as f32);
                self.dirty = true;
            }
            ToolEvent::Button { button, pressed } => {
                let which = match button {
                    BTN_STYLUS => PenButtons::BARREL,
                    BTN_STYLUS2 => PenButtons::SECONDARY_BARREL,
                    // BTN_STYLUS3 and any pad button: no Teksilo button maps
                    // to them, and inventing one would fire a Secondary click
                    // the user did not ask for.
                    _ => return,
                };
                self.buttons = self.buttons.with(which, pressed);
                self.dirty = true;
            }
            ToolEvent::Frame { time_ms } => {
                self.last_frame_ms = Some(time_ms);
                if let Some(packet) = self.commit(our_surface, Some(time_ms)) {
                    out.push(packet);
                }
            }
        }
    }

    /// The packet this `frame` commits, if any, stamped with the frame's own
    /// millisecond time.
    ///
    /// `time_ms` is an `Option` because `Removed` passes
    /// [`Self::last_frame_ms`], which is `None` until the first frame. No
    /// packet is ever built from that `None`: reaching the constructor needs
    /// `reported_proximity`, which only a `Frame` can set, and a `Frame` writes
    /// `last_frame_ms` before it commits. The `None` is carried rather than
    /// defaulted to `0` so that a future path which *did* reach it would
    /// produce an honestly undated packet instead of one claiming the epoch.
    fn commit(&mut self, our_surface: u32, time_ms: Option<u32>) -> Option<PenPacket> {
        if !self.dirty {
            return None;
        }
        self.dirty = false;

        let tool = self.tool?;
        // Not ours, or never was: a tool over a sibling window still frames
        // here, and its state must not leak into this window's stream.
        if self.surface != Some(our_surface) {
            return None;
        }
        // A tool that is out of proximity and was already reported so has
        // nothing left to say.
        if !self.in_proximity && !self.reported_proximity {
            return None;
        }
        self.reported_proximity = self.in_proximity;
        if !self.in_proximity {
            // The session is over; the next proximity_in starts a new one.
            self.surface = None;
        }

        Some(PenPacket {
            tool,
            position: self.position,
            pressure: self.pressure,
            tilt: self.tilt,
            twist: self.twist,
            buttons: self.buttons,
            in_proximity: self.in_proximity,
            down: self.down,
            // `frame`'s millisecond stamp. On the compositor's clock, with no
            // known offset from the tree's epoch — which is exactly why it is
            // carried as a raw counter and read only as a difference within
            // one drained batch. See `PenPacket::device_time_ms`.
            device_time_ms: time_ms,
        })
    }
}

// ---------------------------------------------------------------------------
// The dispatch thread
// ---------------------------------------------------------------------------

/// The packet queue shared between the dispatch thread and the source.
#[derive(Debug, Default)]
struct PenQueue {
    packets: Mutex<Vec<PenPacket>>,
    stop: AtomicBool,
    /// Whether the tablet seat has announced at least one live tool.
    ///
    /// The dispatch thread's only clock: see [`WaylandPenSource::poll_interval`]
    /// for why a session with no tool must not be polled at stylus rate.
    has_tool: AtomicBool,
}

/// Dispatch-thread state.
struct TabletState {
    /// Protocol id of this window's `wl_surface`.
    our_surface: u32,
    queue: Arc<PenQueue>,
    /// One accumulator per live tool, keyed by the tool proxy's object id.
    tools: HashMap<ObjectId, ToolState>,
    /// Held so the proxies outlive the queue.
    _manager: ZwpTabletManagerV2,
    _seat: WlSeat,
    _tablet_seat: ZwpTabletSeatV2,
}

impl TabletState {
    /// Publish whether any tool is live, so the dispatch thread can pick its
    /// poll rate. See [`WaylandPenSource::poll_interval`], which is where the
    /// rule this feeds is tested.
    ///
    /// The *calls* to this, on the seat's `ToolAdded` arm and the tool's own
    /// `Removed` arm, are not witnessed: reaching them needs a compositor
    /// advertising a tablet manager, so deleting either leaves the suite green
    /// while a real stylus is polled at the idle rate. Listed with the other
    /// reviewed-rather-than-tested call sites in `docs/touch-and-pen.md` §9. Listed with the other
    /// reviewed-rather-than-tested call sites in `docs/touch-and-pen.md` §9.
    fn sync_tool_presence(&self) {
        self.queue
            .has_tool
            .store(!self.tools.is_empty(), Ordering::Relaxed);
    }

    fn push(&self, packets: Vec<PenPacket>) {
        if packets.is_empty() {
            return;
        }
        if let Ok(mut queued) = self.queue.packets.lock() {
            queued.extend(packets);
        }
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for TabletState {
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

impl Dispatch<WlSeat, ()> for TabletState {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: <WlSeat as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpTabletManagerV2, ()> for TabletState {
    fn event(
        _: &mut Self,
        _: &ZwpTabletManagerV2,
        _: <ZwpTabletManagerV2 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpTabletV2, ()> for TabletState {
    fn event(
        _: &mut Self,
        _: &ZwpTabletV2,
        _: <ZwpTabletV2 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpTabletSeatV2, ()> for TabletState {
    fn event(
        state: &mut Self,
        _: &ZwpTabletSeatV2,
        event: <ZwpTabletSeatV2 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_tablet_seat_v2::Event::ToolAdded { id } = event {
            state.tools.insert(id.id(), ToolState::default());
            state.sync_tool_presence();
        }
    }

    event_created_child!(TabletState, ZwpTabletSeatV2, [
        zwp_tablet_seat_v2::EVT_TABLET_ADDED_OPCODE => (ZwpTabletV2, ()),
        zwp_tablet_seat_v2::EVT_TOOL_ADDED_OPCODE => (ZwpTabletToolV2, ()),
        zwp_tablet_seat_v2::EVT_PAD_ADDED_OPCODE => (ZwpTabletPadV2, ()),
    ]);
}

impl Dispatch<ZwpTabletToolV2, ()> for TabletState {
    fn event(
        state: &mut Self,
        tool: &ZwpTabletToolV2,
        event: <ZwpTabletToolV2 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(translated) = translate_tool_event(&event) else {
            return;
        };
        let our_surface = state.our_surface;
        let mut out = Vec::new();
        let removed = matches!(translated, ToolEvent::Removed);
        let id = tool.id();
        if let Some(accumulator) = state.tools.get_mut(&id) {
            // `Removed` commits its own leave packet — see `ToolState::apply`.
            accumulator.apply(translated, our_surface, &mut out);
        }
        if removed {
            state.tools.remove(&id);
            state.sync_tool_presence();
        }
        state.push(out);
    }
}

// The pad half of the protocol is not used, but every `new_id` event still
// needs somewhere to land: an unlisted one is a runtime panic in the backend,
// not a silent drop. These impls exist so a tablet with a button pad does not
// take the app down.
impl Dispatch<ZwpTabletPadV2, ()> for TabletState {
    fn event(
        _: &mut Self,
        _: &ZwpTabletPadV2,
        _: <ZwpTabletPadV2 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }

    event_created_child!(TabletState, ZwpTabletPadV2, [
        zwp_tablet_pad_v2::EVT_GROUP_OPCODE => (ZwpTabletPadGroupV2, ()),
    ]);
}

impl Dispatch<ZwpTabletPadGroupV2, ()> for TabletState {
    fn event(
        _: &mut Self,
        _: &ZwpTabletPadGroupV2,
        _: <ZwpTabletPadGroupV2 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }

    event_created_child!(TabletState, ZwpTabletPadGroupV2, [
        zwp_tablet_pad_group_v2::EVT_RING_OPCODE => (ZwpTabletPadRingV2, ()),
        zwp_tablet_pad_group_v2::EVT_STRIP_OPCODE => (ZwpTabletPadStripV2, ()),
    ]);
}

impl Dispatch<ZwpTabletPadRingV2, ()> for TabletState {
    fn event(
        _: &mut Self,
        _: &ZwpTabletPadRingV2,
        _: <ZwpTabletPadRingV2 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpTabletPadStripV2, ()> for TabletState {
    fn event(
        _: &mut Self,
        _: &ZwpTabletPadStripV2,
        _: <ZwpTabletPadStripV2 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// Translate one protocol event into a [`ToolEvent`].
///
/// `None` for the events Teksilo has no home for (`distance`, `slider`,
/// `wheel`, the identity events) — deliberately, and listed in the module docs.
fn translate_tool_event(event: &zwp_tablet_tool_v2::Event) -> Option<ToolEvent> {
    use zwp_tablet_tool_v2::Event as E;
    Some(match event {
        E::Type { tool_type } => ToolEvent::Type(match tool_type {
            WEnum::Value(value) => *value as u32,
            WEnum::Unknown(raw) => *raw,
        }),
        E::ProximityIn { surface, .. } => ToolEvent::ProximityIn {
            surface: surface.id().protocol_id(),
        },
        E::ProximityOut => ToolEvent::ProximityOut,
        E::Down { .. } => ToolEvent::Down,
        E::Up => ToolEvent::Up,
        E::Motion { x, y } => ToolEvent::Motion { x: *x, y: *y },
        E::Pressure { pressure } => ToolEvent::Pressure(*pressure),
        E::Tilt { tilt_x, tilt_y } => ToolEvent::Tilt {
            x: *tilt_x,
            y: *tilt_y,
        },
        E::Rotation { degrees } => ToolEvent::Rotation(*degrees),
        E::Button { button, state, .. } => ToolEvent::Button {
            button: *button,
            pressed: matches!(
                state,
                WEnum::Value(zwp_tablet_tool_v2::ButtonState::Pressed)
            ),
        },
        E::Frame { time } => ToolEvent::Frame { time_ms: *time },
        E::Removed => ToolEvent::Removed,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// The source
// ---------------------------------------------------------------------------

/// A pen source reading `zwp_tablet_v2` for one window.
#[derive(Debug)]
pub struct WaylandPenSource {
    queue: Arc<PenQueue>,
}

/// How long the dispatch thread waits between `dispatch_pending` calls when no
/// tool has been announced.
///
/// See [`WaylandPenSource::poll_interval`] for the whole argument; the number
/// itself is chosen to be far enough below human hot-plug reaction time that a
/// tablet plugged in mid-session is on the fast tier long before its owner can
/// pick the pen up, and far enough above [`POLL_INTERVAL`] that the idle cost
/// is not a timer the kernel has to honour 250 times a second.
const IDLE_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

impl WaylandPenSource {
    /// How long to wait before looking at the queue again.
    ///
    /// This exists because binding the protocol says nothing about the
    /// hardware (see [`Self::attach`]): a compositor advertises
    /// `zwp_tablet_manager_v2` whether or not a digitizer is attached, so on a
    /// modern desktop most windows get one of these threads and most of them
    /// will never see a packet. At [`POLL_INTERVAL`] (4 ms) that thread is 250
    /// timer wakeups a second, for the life of every window, on a machine with
    /// no tablet — which is exactly the kind of idle cost the rest of the
    /// framework is built to avoid.
    ///
    /// The protocol answers it itself. A tablet seat announces `tool_added`
    /// before any tool can be in proximity, so "has a tool ever been
    /// announced" is a sound gate: no tool means no stroke is possible, and
    /// [`IDLE_POLL_INTERVAL`] costs 4 wakeups a second instead of 250. The
    /// moment a tool is announced — at bind time for an already-plugged
    /// tablet, within one idle interval for one plugged in later — the thread
    /// returns to stylus rate, and it drops back when the last tool is
    /// removed.
    ///
    /// The consequence, stated plainly: a tablet hot-plugged mid-session is
    /// noticed up to [`IDLE_POLL_INTERVAL`] late. Nothing is dropped in that
    /// window (the events are buffered in our queue, not discarded) — they are
    /// simply dispatched at the next look, and the next look is a quarter of a
    /// second at worst, before a hand can reach the pen.
    fn poll_interval(has_tool: bool) -> std::time::Duration {
        if has_tool {
            POLL_INTERVAL
        } else {
            IDLE_POLL_INTERVAL
        }
    }

    /// Bind the tablet protocol for `parent`'s window, or `None` when this is
    /// not a Wayland window or the compositor advertises no tablet manager.
    ///
    /// The failure is quiet — the caller falls back to the null source and the
    /// window reports no pen — because it says nothing about the hardware.
    /// Whether `zwp_tablet_manager_v2` is advertised is a **compositor-support
    /// question**, not a proxy for whether a digitizer is plugged in: the
    /// machine this was written on advertises the manager at version 2 with no
    /// digitizer attached at all — checked on a machine where `wayland-info`
    /// reports `zwp_tablet_manager_v2` at version 2 and no input device is a
    /// digitizer.
    /// Every compositor with tablet support advertises the global
    /// unconditionally, so on a modern desktop this returns `Some` on most
    /// machines whether or not a tablet exists — and what decides the *cost*
    /// of that is `Self::poll_interval`, not this. `attach` fails where the
    /// compositor has no tablet support to offer, which today means an older
    /// or a deliberately minimal one.
    pub fn attach(parent: &ParentHandle) -> Option<Self> {
        let RawDisplayHandle::Wayland(display) = parent.raw_display_handle() else {
            return None;
        };
        let RawWindowHandle::Wayland(window) = parent.raw_window_handle() else {
            return None;
        };

        // Wrap winit's existing wl_display: shared connection, shared object
        // id space, so `proximity_in`'s surface id is comparable with ours.
        let backend = unsafe { Backend::from_foreign_display(display.display.as_ptr() as *mut _) };
        let conn = Connection::from_backend(backend);
        let (globals, mut queue) = registry_queue_init::<TabletState>(&conn).ok()?;
        let qh = queue.handle();

        let manager = globals
            .bind::<ZwpTabletManagerV2, _, _>(&qh, 1..=1, ())
            .ok();
        let Some(manager) = manager else {
            trace_input!(
                Samples,
                "pen: the compositor advertises no zwp_tablet_manager_v2"
            );
            return None;
        };
        let seat = globals.bind::<WlSeat, _, _>(&qh, 1..=5, ()).ok()?;
        let tablet_seat = manager.get_tablet_seat(&seat, &qh, ());

        let our_surface = unsafe {
            ObjectId::from_ptr(WlSurface::interface(), window.surface.as_ptr() as *mut _)
        }
        .ok()?
        .protocol_id();

        let shared = Arc::new(PenQueue::default());
        let mut state = TabletState {
            our_surface,
            queue: shared.clone(),
            tools: HashMap::new(),
            _manager: manager,
            _seat: seat,
            _tablet_seat: tablet_seat,
        };

        let thread_queue = shared.clone();
        std::thread::Builder::new()
            .name(format!("teksilo-wayland-pen-{our_surface}"))
            .spawn(move || {
                // CRITICAL: never read the socket. See the module docs.
                while queue.dispatch_pending(&mut state).is_ok() {
                    if thread_queue.stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let _ = conn.flush();
                    std::thread::sleep(Self::poll_interval(
                        thread_queue.has_tool.load(Ordering::Relaxed),
                    ));
                }
            })
            .ok()?;

        Some(Self { queue: shared })
    }
}

impl Drop for WaylandPenSource {
    fn drop(&mut self) {
        self.queue.stop.store(true, Ordering::Relaxed);
    }
}

impl PenSource for WaylandPenSource {
    fn poll(&mut self, out: &mut Vec<PenPacket>) {
        if let Ok(mut packets) = self.queue.packets.lock() {
            out.append(&mut packets);
        }
    }

    fn capabilities(&self) -> PenCaps {
        PenCaps::FULL_PEN
    }

    /// Yes: the tablet listener runs on its own thread — at [`POLL_INTERVAL`]
    /// while a tool is announced, and on a slower tier while none is — so the
    /// event loop has to look again after a wake.
    fn polls_off_thread(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests;
