// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! The pointer vocabulary: who is pointing, when, and with what.
//!
//! Every input sample that reaches the widget tree is described by this
//! module's types. A mouse, a finger and a stylus differ in tuning
//! ([`PointerKind`], which lives in `teksilo-tokens` so a token struct can name
//! it) but not in shape: they all arrive as a [`PointerSample`] carrying a
//! [`PointerInfo`], and they are all timed by one [`EventTime`] measured from
//! one tree epoch.
//!
//! # Identity
//!
//! [`PointerId`] is minted per *press*, not per device, by the process-global
//! [`PointerIdAllocator`]. That is deliberate: winit **reuses** `Touch::id`
//! values once a contact lifts, so a table keyed on the raw OS id can attribute
//! a new contact's samples to the sequence the previous one left behind. A
//! fresh id per Down defeats that without needing a generation counter — the
//! allocator is monotonic, so an id is never handed out twice in one process.
//!
//! The one exception is [`PointerId::MOUSE`], the stable id every synthesized
//! mouse event uses. A mouse is singular by construction, so it needs no
//! per-press identity and legacy call sites can name it without an allocation.
//!
//! # Time
//!
//! [`EventTime`] is a `Duration` since the tree's epoch, never an `Instant`.
//! See [`clock`] for why that matters and for the one-clock rule.
//!
//! Reference: `docs/touch-and-pen.md`.

pub mod clock;
pub mod hit_slop;
pub mod table;
pub mod touch_action;
pub mod trace;

use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use teksilo_canvas::{Point, Size};
use teksilo_tokens::PointerKind;

use crate::event::{ButtonMask, Modifiers, PointerButton, ScrollDelta};

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

/// A process-unique, monotonically increasing pointer identity.
///
/// Minted per press by [`PointerIdAllocator`] (see the module docs for why per
/// press rather than per device). `NonZeroU64` so `Option<PointerId>` is the
/// same size as `PointerId`, and so id `0` can never be confused with "no
/// pointer".
///
/// Ordering is by mint order, which makes a `BTreeMap<PointerId, _>` iterate
/// oldest contact first — the order a multi-touch consumer wants.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct PointerId(NonZeroU64);

impl PointerId {
    /// The one stable id every synthesized mouse event uses.
    ///
    /// A mouse is singular: there is at most one of it, it never lifts, and
    /// nothing about it needs a per-press identity. Reserving id 1 for it keeps
    /// the legacy `dispatch_event` path allocation-free and makes "is this the
    /// mouse?" a comparison rather than a lookup.
    pub const MOUSE: Self = Self(NonZeroU64::new(1).unwrap());

    /// The raw value, for a backend that must store the id compactly or hand
    /// it to a C API. Never construct a `PointerId` from one of these — only
    /// the allocator may mint.
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// An opaque per-device key, derived by the platform layer from the backend's
/// own device handle (winit's `DeviceId`, a Win32 `HANDLE`, an evdev node).
///
/// Two different devices can report the same OS-level contact id at the same
/// time — two touchscreens, or a touchscreen and a digitizer — so the
/// allocator's live table is keyed on `(device, os_id)` rather than `os_id`
/// alone. Teksilo never interprets the value; it only needs it to be stable for
/// the life of a device and distinct between devices.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct BackendDeviceKey(u64);

impl BackendDeviceKey {
    /// The key a backend with no device concept uses (a single-touchscreen
    /// platform, a test harness).
    pub const DEFAULT: Self = Self(0);

    /// Wrap a backend-derived value.
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    /// The wrapped value.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Mints [`PointerId`]s and maps a backend's reused contact ids onto them.
///
/// One instance per process, reached through [`PointerIdAllocator::global`].
/// The mapping is `(device, os_id) -> PointerId`, established at [`begin`] and
/// torn down at [`end`]; [`get`] resolves the samples in between.
///
/// [`begin`]: Self::begin
/// [`end`]: Self::end
/// [`get`]: Self::get
#[derive(Debug)]
pub struct PointerIdAllocator {
    /// Next id to hand out. Starts past [`PointerId::MOUSE`].
    next: AtomicU64,
    /// Live `(device, os_id) -> id` mappings, one per contact currently down.
    ///
    /// A `Mutex` rather than a `RefCell` because the platform layer may mint
    /// from a backend thread (X11's XDND helper connection already runs on
    /// one). It is never held across a dispatch, so it is always uncontended
    /// in practice.
    live: Mutex<HashMap<(BackendDeviceKey, u64), PointerId>>,
}

/// The one allocator. Not `pub`: reached through
/// [`PointerIdAllocator::global`].
static GLOBAL_ALLOCATOR: OnceLock<PointerIdAllocator> = OnceLock::new();

impl PointerIdAllocator {
    /// The process-global allocator.
    pub fn global() -> &'static Self {
        GLOBAL_ALLOCATOR.get_or_init(|| Self {
            // 1 is `PointerId::MOUSE`; real pointers start at 2.
            next: AtomicU64::new(2),
            live: Mutex::new(HashMap::new()),
        })
    }

    /// Mint a fresh id for a press and remember it for `(device, os_id)`.
    ///
    /// A second `begin` on a key that is already live *replaces* the mapping
    /// and returns a new id — a backend that drops an Up (a lost contact, a
    /// window that stopped receiving events mid-gesture) must not strand the
    /// next press on the old identity.
    pub fn begin(&self, device: BackendDeviceKey, os_id: u64) -> PointerId {
        let raw = self.next.fetch_add(1, Ordering::Relaxed);
        let id = PointerId(NonZeroU64::new(raw).expect("allocator starts at 2 and only grows"));
        if let Ok(mut live) = self.live.lock() {
            live.insert((device, os_id), id);
        }
        id
    }

    /// The id minted for a live `(device, os_id)`, if the contact is still
    /// down. `None` after [`end`](Self::end), which is what makes a reused OS
    /// id resolve to a *new* [`PointerId`] rather than the stale one.
    pub fn get(&self, device: BackendDeviceKey, os_id: u64) -> Option<PointerId> {
        self.live
            .lock()
            .ok()
            .and_then(|live| live.get(&(device, os_id)).copied())
    }

    /// Forget the mapping for a lifted contact. Idempotent; returns the id that
    /// was live, if any.
    pub fn end(&self, device: BackendDeviceKey, os_id: u64) -> Option<PointerId> {
        self.live
            .lock()
            .ok()
            .and_then(|mut live| live.remove(&(device, os_id)))
    }

    /// Number of contacts currently mapped. Test/diagnostic helper.
    pub fn live_count(&self) -> usize {
        self.live.lock().map(|live| live.len()).unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Time
// ---------------------------------------------------------------------------

/// A moment on the input timeline, measured from the tree's epoch.
///
/// A `Duration`, never an `Instant`: a recognizer that reads `Instant::now()`
/// cannot be driven by a simulated clock, and a test that cannot advance the
/// clock cannot test a long press, a fling or a double-tap window without
/// sleeping. Every deadline in the gesture layer is expressed as an
/// `EventTime`, and the tree's one [`InputClock`](clock::InputClock) is the
/// only thing that produces one.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct EventTime(Duration);

impl EventTime {
    /// The epoch itself.
    pub const ZERO: Self = Self(Duration::ZERO);

    /// A time this far after the epoch.
    pub const fn from_duration(d: Duration) -> Self {
        Self(d)
    }

    /// Milliseconds after the epoch. Convenience for tests and for backends
    /// whose timestamps arrive in milliseconds.
    pub const fn from_millis(ms: u64) -> Self {
        Self(Duration::from_millis(ms))
    }

    /// How far this is after the epoch.
    pub const fn as_duration(self) -> Duration {
        self.0
    }

    /// How long after `earlier` this is, saturating at zero.
    ///
    /// Saturating rather than panicking because samples can arrive out of
    /// order: a backend that batches coalesced moves may hand over a packet
    /// whose timestamps precede the last one already processed, and a gesture
    /// must degrade to "no time passed" rather than abort.
    pub const fn saturating_since(self, earlier: Self) -> Duration {
        self.0.saturating_sub(earlier.0)
    }

    /// This time plus `d`, or `None` on overflow.
    ///
    /// Deadlines are computed this way (`now.checked_add(long_press)`), so the
    /// overflow case is real rather than theoretical for a caller that passes
    /// `Duration::MAX` to mean "never".
    pub fn checked_add(self, d: Duration) -> Option<Self> {
        self.0.checked_add(d).map(Self)
    }
}

impl std::ops::Add<Duration> for EventTime {
    type Output = Self;

    /// `now + d`, saturating at [`Duration::MAX`].
    ///
    /// Saturating rather than panicking for the same reason
    /// [`checked_add`](EventTime::checked_add) exists: `Duration::MAX` is a
    /// legitimate way to say "never", and a deadline arithmetic panic in the
    /// middle of a gesture would be absurd. Reach for `checked_add` where the
    /// overflow itself has to be observed.
    fn add(self, d: Duration) -> Self {
        Self(self.0.saturating_add(d))
    }
}

// ---------------------------------------------------------------------------
// Sample payloads
// ---------------------------------------------------------------------------

/// The continuous per-sample axes a device may report beyond position.
///
/// Every field is optional because every field is optional in the hardware: a
/// mouse reports none of them, a touchscreen usually reports a contact patch
/// and sometimes a pressure, a good digitizer reports all five.
///
/// `#[non_exhaustive]`: barrel rotation, hover distance and per-axis tilt
/// resolution are all plausible additions.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct PointerAxes {
    /// Normalised tip pressure, `0.0..=1.0`. See
    /// [`PointerInfo::effective_pressure`] for the value a consumer should
    /// actually read.
    pub pressure: Option<f32>,
    /// Normalised barrel-button pressure, `0.0..=1.0` (or `-1.0..=1.0` for a
    /// device with a centred rest position), as in the W3C Pointer Events
    /// `tangentialPressure`.
    pub tangential_pressure: Option<f32>,
    /// Stylus tilt as `(tilt_x, tilt_y)` in degrees, each `-90.0..=90.0`.
    pub tilt: Option<(f32, f32)>,
    /// Stylus barrel rotation in degrees, `0.0..=359.0`.
    pub twist: Option<f32>,
    /// The size of the contact patch in logical pixels. A finger's ellipse; a
    /// palm's is what a rejection heuristic reads.
    pub contact: Option<Size>,
}

/// Everything that identifies and describes the pointer producing a sample.
///
/// Carried by [`PointerSample`], [`ScrollSample`] and (from stage 1 of the
/// event-shape landing) by the scroll and cancel `WidgetEvent`s.
///
/// `#[non_exhaustive]`: construct one through [`mouse`](Self::mouse) /
/// [`touch`](Self::touch) and adjust fields, so a later field cannot break a
/// call site.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PointerInfo {
    /// Which pointer this is. See the module docs on per-press identity.
    pub id: PointerId,
    /// What kind of device it is — the axis gesture tuning reads.
    pub kind: PointerKind,
    /// Whether this is the *primary* pointer in the W3C sense: the one that
    /// drives the legacy singular signals (`hovered`, the cursor, the one
    /// `PointerDown` a widget that knows nothing of multi-touch will see).
    /// Exactly one live pointer is primary; a mouse always wins the role.
    pub primary: bool,
    /// The buttons held *after* this sample is applied. A press sets its own
    /// bit; a release clears it. Empty for a hovering pointer.
    pub buttons: ButtonMask,
    /// The continuous axes, where the device reports them.
    pub axes: PointerAxes,
    /// When the backend says this sample happened, on the tree's timeline.
    pub time: EventTime,
    /// The digitizer classified this contact as a palm rather than a
    /// deliberate touch.
    ///
    /// Only a backend that advertises `reports_palm` ever sets it; everything
    /// else leaves it `false`, which is why no existing call site changes
    /// meaning. A flagged sample is refused by
    /// [`PointerTable::begin`](table::PointerTable::begin) and never reaches a
    /// widget: rejecting it at the table is what keeps a hand resting on a
    /// tablet from opening menus, and it is one decision rather than one per
    /// recognizer.
    pub palm: bool,
}

impl PointerInfo {
    /// The mouse: [`PointerId::MOUSE`], [`PointerKind::Mouse`], primary, no
    /// buttons held, no axes.
    ///
    /// This is what every legacy `WidgetEvent` constructor defaults to, so a
    /// call site that says nothing about pointers keeps meaning exactly what it
    /// meant before the touch programme.
    pub const fn mouse(time: EventTime) -> Self {
        Self {
            id: PointerId::MOUSE,
            kind: PointerKind::Mouse,
            primary: true,
            buttons: ButtonMask::NONE,
            axes: PointerAxes {
                pressure: None,
                tangential_pressure: None,
                tilt: None,
                twist: None,
                contact: None,
            },
            time,
            palm: false,
        }
    }

    /// A touch contact. Not primary by default: primacy is decided by the
    /// pointer table against the other live pointers, not by the constructor.
    pub const fn touch(id: PointerId, time: EventTime) -> Self {
        Self {
            id,
            kind: PointerKind::Touch,
            primary: false,
            buttons: ButtonMask::NONE,
            axes: PointerAxes {
                pressure: None,
                tangential_pressure: None,
                tilt: None,
                twist: None,
                contact: None,
            },
            time,
            palm: false,
        }
    }

    /// Whether the user points at the pixel directly — see
    /// [`PointerKind::is_direct`].
    pub const fn is_direct(&self) -> bool {
        self.kind.is_direct()
    }

    /// Whether the contact patch is large enough that the reported point is an
    /// estimate — see [`PointerKind::is_coarse`].
    pub const fn is_coarse(&self) -> bool {
        self.kind.is_coarse()
    }

    /// Whether the reported position is accurate to about a pixel — see
    /// [`PointerKind::is_precise`].
    pub const fn is_precise(&self) -> bool {
        self.kind.is_precise()
    }

    /// The pressure a consumer should read: the reported value if the device
    /// gave one, else `0.5` while any button is down, else `0.0`.
    ///
    /// This is the W3C Pointer Events Level 3 rule for `pressure`, and it
    /// exists so a pressure-sensitive surface (a brush, a force-touch
    /// affordance) has a defined answer for a mouse without special-casing it.
    pub fn effective_pressure(&self) -> f32 {
        match self.axes.pressure {
            Some(p) => p,
            None if !self.buttons.is_empty() => 0.5,
            None => 0.0,
        }
    }
}

/// Which end of a pointer's life a sample sits at.
///
/// `Cancel` is not an Up: an Up means the user completed the interaction, a
/// Cancel means the system took it away (see [`CancelReason`]). Conflating them
/// is how a drag whose window lost focus ends up "dropped" where the pointer
/// happened to be.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum PointerPhase {
    /// The pointer came down / a button went down.
    Down,
    /// The pointer moved (with or without buttons held).
    Move,
    /// The pointer lifted / a button went up.
    Up,
    /// The system revoked the pointer. Carries no meaningful end position.
    Cancel,
}

/// One pointer sample as it enters the tree.
///
/// The unit [`WidgetTree::dispatch_pointer`](crate::WidgetTree::dispatch_pointer)
/// consumes. A backend produces exactly one of these per OS packet, folding any
/// intermediate positions the OS batched into [`coalesced`](Self::coalesced).
#[derive(Clone, Debug)]
pub struct PointerSample {
    /// Who is pointing.
    pub pointer: PointerInfo,
    /// What happened.
    pub phase: PointerPhase,
    /// Where, in window-logical coordinates.
    pub position: Point,
    /// The button that changed on a [`Down`](PointerPhase::Down) or
    /// [`Up`](PointerPhase::Up). `None` for a move, a cancel, and for a
    /// buttonless direct-pointer contact.
    pub button: Option<PointerButton>,
    /// Modifier keys held when the sample was produced.
    pub modifiers: Modifiers,
    /// Positions the OS batched into this packet, oldest first, *excluding*
    /// [`position`](Self::position) (which is the newest).
    ///
    /// A velocity tracker integrates over these; a drawing surface draws
    /// through them; everything else ignores them. Deliberately a `Vec` and not
    /// a `SmallVec`: this costs one allocation per packet that actually
    /// coalesced, and `teksilo-core`'s dependency set is small on purpose.
    pub coalesced: Vec<(EventTime, Point, PointerAxes)>,
}

impl PointerSample {
    /// A mouse sample with no coalesced history — the shape every legacy
    /// `WidgetEvent` lowers to.
    pub fn mouse(phase: PointerPhase, position: Point, time: EventTime) -> Self {
        Self {
            pointer: PointerInfo::mouse(time),
            phase,
            position,
            button: None,
            modifiers: Modifiers::NONE,
            coalesced: Vec::new(),
        }
    }

    /// This sample with `button` recorded as the button that changed.
    pub fn with_button(mut self, button: PointerButton) -> Self {
        self.button = Some(button);
        self
    }

    /// This sample with `modifiers` recorded.
    pub fn with_modifiers(mut self, modifiers: Modifiers) -> Self {
        self.modifiers = modifiers;
        self
    }
}

// ---------------------------------------------------------------------------
// Scroll
// ---------------------------------------------------------------------------

/// Where a scroll sits in a continuous gesture.
///
/// A wheel notch is [`Discrete`](Self::Discrete) — it has no beginning and no
/// end — which is why that is the default and why nothing changes for a mouse.
/// A trackpad gesture and a synthesised touch pan run
/// `Began → Changed* → Ended`, optionally followed by `Momentum* →
/// MomentumEnded` while the content coasts.
///
/// `#[non_exhaustive]`: a rubber-band settle phase is anticipated.
#[non_exhaustive]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum ScrollPhase {
    /// A self-contained scroll with no phase structure — a wheel notch. The
    /// default, and what every scroll in Teksilo was before the touch
    /// programme.
    #[default]
    Discrete,
    /// The user's fingers went down and the gesture began.
    Began,
    /// The gesture is in progress.
    Changed,
    /// The user's fingers lifted. Any momentum follows separately.
    Ended,
    /// The content is coasting after the fingers lifted.
    Momentum,
    /// The coast finished.
    MomentumEnded,
    /// A one-shot flick with a release velocity, for backends that report a
    /// fling rather than a momentum stream.
    Fling,
    /// The gesture was revoked before it ended.
    Cancelled,
}

/// What produced a scroll.
///
/// Read by a consumer that must treat a precise pixel stream differently from a
/// notched wheel — the classic case being "one wheel notch = one item" versus
/// "follow the trackpad exactly".
#[non_exhaustive]
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum ScrollSource {
    /// A notched mouse wheel. The default.
    #[default]
    Wheel,
    /// A precision trackpad or a free-spinning wheel.
    Trackpad,
    /// A pan gesture synthesised from a direct pointer dragging the content.
    TouchPan,
    /// The app scrolled itself (a keyboard command, `ensure_visible`, an
    /// animation).
    Programmatic,
}

/// One scroll sample as it enters the tree.
///
/// The unit [`WidgetTree::dispatch_scroll`](crate::WidgetTree::dispatch_scroll)
/// consumes.
#[derive(Clone, Debug)]
pub struct ScrollSample {
    /// How far to scroll, in lines or pixels.
    pub delta: ScrollDelta,
    /// Where the pointer was, in window-logical coordinates, when the scroll
    /// happened.
    ///
    /// `Some` routes the scroll by hit test; `None` falls back to the hovered
    /// (else focused) widget. A wheel event has historically been `None` and
    /// stays that way, so a mouse routes exactly as before; a synthesised touch
    /// pan **must** carry a position, because a contact never writes hover and
    /// would otherwise route nowhere.
    pub position: Option<Point>,
    /// Where in a continuous gesture this sample sits.
    pub phase: ScrollPhase,
    /// What produced it.
    pub source: ScrollSource,
    /// Who is pointing.
    pub pointer: PointerInfo,
    /// Modifier keys held when the sample was produced. Ctrl-wheel-to-zoom
    /// reads this.
    pub modifiers: Modifiers,
}

impl ScrollSample {
    /// A discrete wheel notch from the mouse, routed by hover — exactly what
    /// `WidgetEvent::Scroll` meant before the touch programme.
    pub fn wheel(delta: ScrollDelta, modifiers: Modifiers, time: EventTime) -> Self {
        Self {
            delta,
            position: None,
            phase: ScrollPhase::Discrete,
            source: ScrollSource::Wheel,
            pointer: PointerInfo::mouse(time),
            modifiers,
        }
    }

    /// This sample routed at `position` rather than by hover.
    pub fn at(mut self, position: Point) -> Self {
        self.position = Some(position);
        self
    }
}

// ---------------------------------------------------------------------------
// Cancellation
// ---------------------------------------------------------------------------

/// Why a pointer interaction was revoked.
///
/// Declared in full here so the taxonomy is one enumeration rather than a
/// growing set of booleans, and so a consumer can `match` on it exhaustively.
/// Every variant reaches a widget through the one funnel,
/// [`WidgetTree::cancel_pointer`](crate::WidgetTree::cancel_pointer), and is
/// delivered as a [`WidgetEvent::PointerCancel`](crate::event::WidgetEvent::PointerCancel).
/// A handful name a producer whose own package has not landed and are marked
/// as such below; `docs/touch-and-pen.md` §3.3 carries the full table of who
/// raises each, who receives it, and what the widget must do about it.
///
/// `#[non_exhaustive]`: the taxonomy is expected to grow as backends reveal
/// revocation paths Teksilo has not met.
#[non_exhaustive]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum CancelReason {
    /// The OS itself revoked the pointer (a `PointerCaptureLost`, a
    /// `wl_touch.cancel`, a compositor grab).
    Platform,
    /// The window lost focus mid-interaction.
    WindowDeactivated,
    /// The window became fully occluded mid-interaction. Reserved for the
    /// platform layer's occlusion path.
    Occluded,
    /// A modal surface opened over the interaction.
    ModalOpened,
    /// The interacting subtree went dormant (a `Switcher` branch was parked, a
    /// tab was switched away from).
    SubtreeParked,
    /// The interacting widget was destroyed.
    WidgetDestroyed,
    /// The widget holding the pointer capture went away, leaving the capture
    /// with no owner.
    CaptureOrphaned,
    /// A native OS drag started from this press, so the in-app interaction ends.
    OsDragStarted,
    /// An external (OS) drag-and-drop session took the pointer over. Reserved
    /// for the inbound external-DnD path.
    ExternalDndTakeover,
    /// Another member of the gesture sequence won arbitration, so this one is
    /// revoked.
    PeerClaimed,
    /// The overlay the interaction lived in was dismissed under it.
    OverlayDismissed,
    /// A second contact arrived on a surface that handles only one, so the
    /// interaction is abandoned rather than misread. Reserved for
    /// `MultiContact::First`.
    MultiContactIgnored,
    /// More simultaneous contacts arrived than the pointer table holds.
    /// Refused at [`PointerTable::begin`](crate::pointer::table::PointerTable::begin),
    /// before any event exists, so no widget is told.
    ContactCapExceeded,
    /// The contact was classified as a palm rather than a deliberate touch.
    /// Refused at [`PointerTable::begin`](crate::pointer::table::PointerTable::begin),
    /// before any event exists, so no widget is told.
    PalmRejected,
    /// A catch-all for a deactivation that fits none of the above. Prefer a
    /// specific variant; this one exists so a caller is never forced to lie.
    Deactivated,
}

// ---------------------------------------------------------------------------
// Per-dispatch snapshot
// ---------------------------------------------------------------------------

/// What the tree knows about the sample currently being dispatched.
///
/// Snapshotted onto every [`EventContext`](crate::widget::EventContext) so a
/// handler can ask which pointer it is serving without the answer having to be
/// threaded through every handler signature.
///
/// The two obvious producers are a pointer sample
/// ([`from_pointer_sample`](Self::from_pointer_sample)) and a scroll sample
/// ([`from_scroll_sample`](Self::from_scroll_sample)). The two a reader is
/// likely to get wrong are the ones with a pointer but **no sample**: a gesture
/// the *timer* recognised ([`for_recognized_gesture`](Self::for_recognized_gesture))
/// — a hold — and a **drag session** ([`for_drag_session`](Self::for_drag_session)),
/// whose ticks fire from a layout pass and whose OS phases arrive from a platform
/// thread. Everything else — a legacy `WidgetEvent`
/// ([`from_event`](Self::from_event)), an accessibility action, a hand-built test
/// context — holds the [`Default`], a mouse at the epoch.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct InputSnapshot {
    pub(crate) pointer: PointerInfo,
    pub(crate) position: Option<Point>,
    pub(crate) scroll_phase: ScrollPhase,
    pub(crate) scroll_source: ScrollSource,
    /// The positions the OS batched into this packet, oldest first and
    /// excluding [`position`](Self::position).
    ///
    /// Carried onto the snapshot — rather than left on the
    /// [`PointerSample`] the dispatcher discards — because the velocity fit
    /// behind a fling has to see them: a 500 Hz digitiser decimated to frame
    /// rate under-reads a flick by the ratio of the two rates. Empty for every
    /// producer that does not coalesce, which costs no allocation.
    pub(crate) coalesced: Vec<(EventTime, Point)>,
}

impl Default for InputSnapshot {
    fn default() -> Self {
        Self {
            pointer: PointerInfo::mouse(EventTime::ZERO),
            position: None,
            scroll_phase: ScrollPhase::Discrete,
            scroll_source: ScrollSource::Wheel,
            coalesced: Vec::new(),
        }
    }
}

impl InputSnapshot {
    /// The snapshot a pointer sample implies.
    pub(crate) fn from_pointer_sample(sample: &PointerSample) -> Self {
        Self {
            pointer: sample.pointer,
            position: Some(sample.position),
            coalesced: sample
                .coalesced
                .iter()
                .map(|&(time, point, _)| (time, point))
                .collect(),
            ..Self::default()
        }
    }

    /// The snapshot a gesture recognised by the **timer** implies.
    ///
    /// A hold is not a sample: nothing arrived, a deadline came due. But it is
    /// still one contact's gesture, and a handler reached from it must not be
    /// told it is serving the mouse — which is what it was told for as long as
    /// this constructor did not exist, because `current_input` is
    /// saved-and-restored around every dispatch and so holds the
    /// [`Default`](Self::default) by the time a timer runs.
    ///
    /// [`position`](Self::position) stays `None` on purpose. The gesture
    /// carries its own position, in **widget-local** coordinates, on the event
    /// the handler is given; publishing a window position here as well would
    /// offer a handler two answers that do not agree.
    pub(crate) fn for_recognized_gesture(pointer: PointerInfo) -> Self {
        Self {
            pointer,
            ..Self::default()
        }
    }

    /// The snapshot a **drag session** implies.
    ///
    /// A drag-and-drop session outlives the sample that started it: `on_drag_tick`
    /// fires from a layout pass, and an OS drag's phases arrive from a platform
    /// thread. Neither is a sample, so `current_input` holds the
    /// [`Default`](Self::default) there — and a drag handler asking which device
    /// it is serving was told "mouse" for the whole of a finger drag. The tree
    /// installs this around those dispatches instead; the pointer comes from
    /// `DragSession::pointer`, recorded when the drag started.
    ///
    /// [`position`](Self::position) stays `None` for the same reason it does on
    /// [`for_recognized_gesture`](Self::for_recognized_gesture): the drag
    /// handler is handed its position in **widget-local** coordinates, and a
    /// window position published beside it would be a second answer that
    /// disagrees.
    pub(crate) fn for_drag_session(pointer: PointerInfo) -> Self {
        Self {
            pointer,
            ..Self::default()
        }
    }

    /// The snapshot a scroll sample implies.
    pub(crate) fn from_scroll_sample(sample: &ScrollSample) -> Self {
        Self {
            pointer: sample.pointer,
            position: sample.position,
            scroll_phase: sample.phase,
            scroll_source: sample.source,
            coalesced: Vec::new(),
        }
    }

    /// The snapshot a legacy [`WidgetEvent`](crate::event::WidgetEvent)
    /// implies. Pointer-bearing variants report what they carry; everything
    /// else reports the default mouse.
    pub(crate) fn from_event(event: &crate::event::WidgetEvent) -> Self {
        use crate::event::WidgetEvent;
        match event {
            WidgetEvent::PointerDown { position, .. }
            | WidgetEvent::PointerUp { position, .. }
            | WidgetEvent::PointerMove { position } => Self {
                position: Some(*position),
                ..Self::default()
            },
            WidgetEvent::Scroll {
                position,
                phase,
                pointer,
                ..
            } => Self {
                pointer: *pointer,
                position: *position,
                scroll_phase: *phase,
                // A legacy `Scroll` carries no source; a wheel notch is what it
                // has always been. `dispatch_scroll` overrides this from the
                // sample.
                scroll_source: ScrollSource::Wheel,
                coalesced: Vec::new(),
            },
            WidgetEvent::PointerCancel {
                position, pointer, ..
            } => Self {
                pointer: *pointer,
                position: *position,
                ..Self::default()
            },
            _ => Self::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- EventTime -------------------------------------------------------

    #[test]
    fn event_time_measures_from_the_epoch() {
        let t = EventTime::from_millis(250);
        assert_eq!(t.as_duration(), Duration::from_millis(250));
        assert_eq!(EventTime::ZERO.as_duration(), Duration::ZERO);
        assert_eq!(EventTime::default(), EventTime::ZERO);
    }

    #[test]
    fn saturating_since_measures_forward() {
        let a = EventTime::from_millis(100);
        let b = EventTime::from_millis(350);
        assert_eq!(b.saturating_since(a), Duration::from_millis(250));
        assert_eq!(a.saturating_since(a), Duration::ZERO);
    }

    /// Samples can arrive out of order (a coalesced packet whose timestamps
    /// predate the last one processed). An inverted pair must read as "no time
    /// passed", not underflow.
    #[test]
    fn saturating_since_clamps_an_inverted_pair() {
        let early = EventTime::from_millis(10);
        let late = EventTime::from_millis(900);
        assert_eq!(early.saturating_since(late), Duration::ZERO);
    }

    #[test]
    fn checked_add_reports_overflow() {
        let t = EventTime::from_millis(5);
        assert_eq!(
            t.checked_add(Duration::from_millis(15)),
            Some(EventTime::from_millis(20))
        );
        assert_eq!(t.checked_add(Duration::MAX), None);
    }

    #[test]
    fn event_times_order_by_their_offset() {
        let mut times = [
            EventTime::from_millis(30),
            EventTime::ZERO,
            EventTime::from_millis(7),
        ];
        times.sort();
        assert_eq!(
            times,
            [
                EventTime::ZERO,
                EventTime::from_millis(7),
                EventTime::from_millis(30)
            ]
        );
    }

    // --- PointerId -------------------------------------------------------

    /// The reason there is no generation field: winit **reuses** `Touch::id`.
    /// A press, a lift and a second press on the same raw id must produce two
    /// different `PointerId`s, or the second contact inherits the first's
    /// sequence.
    #[test]
    fn a_reused_os_id_mints_a_fresh_pointer_id() {
        let alloc = PointerIdAllocator::global();
        let device = BackendDeviceKey::new(0xFEED);

        let first = alloc.begin(device, 7);
        assert_eq!(alloc.get(device, 7), Some(first));
        assert_eq!(alloc.end(device, 7), Some(first));
        assert_eq!(alloc.get(device, 7), None);

        let second = alloc.begin(device, 7);
        assert_ne!(first, second, "a reused OS id must not reuse the PointerId");
        assert!(second > first, "ids are monotonic");
        alloc.end(device, 7);
    }

    /// Two devices may report the same contact id at the same time.
    #[test]
    fn the_same_os_id_on_two_devices_is_two_pointers() {
        let alloc = PointerIdAllocator::global();
        let screen = BackendDeviceKey::new(0xA1);
        let tablet = BackendDeviceKey::new(0xB2);

        let a = alloc.begin(screen, 1);
        let b = alloc.begin(tablet, 1);
        assert_ne!(a, b);
        assert_eq!(alloc.get(screen, 1), Some(a));
        assert_eq!(alloc.get(tablet, 1), Some(b));

        alloc.end(screen, 1);
        assert_eq!(alloc.get(tablet, 1), Some(b), "ending one leaves the other");
        alloc.end(tablet, 1);
    }

    /// A backend that loses an Up must not strand the next press on the stale
    /// identity.
    #[test]
    fn a_second_begin_replaces_a_stranded_mapping() {
        let alloc = PointerIdAllocator::global();
        let device = BackendDeviceKey::new(0xC3);
        let first = alloc.begin(device, 42);
        let second = alloc.begin(device, 42);
        assert_ne!(first, second);
        assert_eq!(alloc.get(device, 42), Some(second));
        alloc.end(device, 42);
    }

    #[test]
    fn ending_an_unknown_contact_is_a_no_op() {
        let alloc = PointerIdAllocator::global();
        assert_eq!(alloc.end(BackendDeviceKey::new(0xD4), 999), None);
    }

    #[test]
    fn the_mouse_id_is_never_minted() {
        let alloc = PointerIdAllocator::global();
        let device = BackendDeviceKey::new(0xE5);
        let id = alloc.begin(device, 3);
        assert_ne!(id, PointerId::MOUSE);
        assert_eq!(PointerId::MOUSE.get(), 1);
        alloc.end(device, 3);
    }

    // --- PointerInfo -----------------------------------------------------

    #[test]
    fn the_mouse_constructor_is_the_legacy_pointer() {
        let m = PointerInfo::mouse(EventTime::ZERO);
        assert_eq!(m.id, PointerId::MOUSE);
        assert_eq!(m.kind, PointerKind::Mouse);
        assert!(m.primary);
        assert!(m.buttons.is_empty());
        assert_eq!(m.axes, PointerAxes::default());
        assert!(!m.is_direct() && !m.is_coarse() && m.is_precise());
    }

    #[test]
    fn a_touch_contact_is_direct_and_coarse() {
        let t = PointerInfo::touch(PointerId::MOUSE, EventTime::ZERO);
        assert_eq!(t.kind, PointerKind::Touch);
        assert!(t.is_direct() && t.is_coarse() && !t.is_precise());
        assert!(
            !t.primary,
            "primacy is the pointer table's decision, not the constructor's"
        );
    }

    /// W3C Pointer Events L3: report what the device said; failing that, 0.5
    /// while a button is down and 0.0 otherwise.
    #[test]
    fn effective_pressure_follows_the_w3c_rule() {
        let mut m = PointerInfo::mouse(EventTime::ZERO);
        assert_eq!(m.effective_pressure(), 0.0);

        m.buttons = ButtonMask::PRIMARY;
        assert_eq!(m.effective_pressure(), 0.5);

        m.axes.pressure = Some(0.75);
        assert_eq!(m.effective_pressure(), 0.75);

        m.buttons = ButtonMask::NONE;
        assert_eq!(m.effective_pressure(), 0.75, "a reported value always wins");
    }

    // --- Samples ---------------------------------------------------------

    #[test]
    fn a_mouse_sample_carries_no_coalesced_history() {
        let s = PointerSample::mouse(PointerPhase::Down, Point::new(3.0, 4.0), EventTime::ZERO)
            .with_button(PointerButton::Primary)
            .with_modifiers(Modifiers::SHIFT);
        assert!(s.coalesced.is_empty());
        assert_eq!(s.button, Some(PointerButton::Primary));
        assert_eq!(s.modifiers, Modifiers::SHIFT);
        assert_eq!(s.pointer.id, PointerId::MOUSE);
    }

    #[test]
    fn a_wheel_sample_is_discrete_and_positionless() {
        let s = ScrollSample::wheel(
            ScrollDelta::Lines { x: 0.0, y: -1.0 },
            Modifiers::NONE,
            EventTime::ZERO,
        );
        assert_eq!(s.phase, ScrollPhase::Discrete);
        assert_eq!(s.source, ScrollSource::Wheel);
        assert_eq!(s.position, None);

        let at = s.at(Point::new(10.0, 20.0));
        assert_eq!(at.position, Some(Point::new(10.0, 20.0)));
    }

    #[test]
    fn scroll_defaults_are_todays_wheel() {
        assert_eq!(ScrollPhase::default(), ScrollPhase::Discrete);
        assert_eq!(ScrollSource::default(), ScrollSource::Wheel);
    }

    // --- InputSnapshot ---------------------------------------------------

    #[test]
    fn the_default_snapshot_is_a_mouse_at_the_epoch() {
        let s = InputSnapshot::default();
        assert_eq!(s.pointer.id, PointerId::MOUSE);
        assert_eq!(s.pointer.time, EventTime::ZERO);
        assert_eq!(s.position, None);
        assert_eq!(s.scroll_phase, ScrollPhase::Discrete);
        assert_eq!(s.scroll_source, ScrollSource::Wheel);
    }

    #[test]
    fn a_scroll_sample_snapshot_keeps_its_phase_and_source() {
        let sample = ScrollSample {
            delta: ScrollDelta::Pixels { x: 0.0, y: 12.0 },
            position: Some(Point::new(5.0, 5.0)),
            phase: ScrollPhase::Momentum,
            source: ScrollSource::TouchPan,
            pointer: PointerInfo::mouse(EventTime::from_millis(9)),
            modifiers: Modifiers::NONE,
        };
        let snap = InputSnapshot::from_scroll_sample(&sample);
        assert_eq!(snap.scroll_phase, ScrollPhase::Momentum);
        assert_eq!(snap.scroll_source, ScrollSource::TouchPan);
        assert_eq!(snap.position, Some(Point::new(5.0, 5.0)));
    }
}
