// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! winit packets in, Teksilo input samples out.
//!
//! [`TranslationState`] is the per-window owner of everything a translation
//! needs to remember: the scale factor, the modifier set, the mouse cursor's
//! last position, the **live contact set**, the scroll-phase machine, and the
//! suppressors that keep a dual-stream platform from delivering one physical
//! touch twice.
//!
//! # Two surfaces, one state
//!
//! The free `translate_*` functions are the original single-`WidgetEvent`
//! surface the app event loop uses today. [`PointerBackend::translate`] is the
//! multi-sample surface that carries touch. Both read the same
//! [`TranslationState`], so the suppressors cannot disagree between them.
//!
//! Mouse translation through the free functions is unchanged, with one
//! deliberate exception: a `MouseInput` that arrives with **no known cursor
//! position** is now dropped rather than dispatched at the window origin. A
//! press at `(0, 0)` is a click on whatever happens to be in the top-left
//! corner, which is worse than no click at all.
//!
//! # Time
//!
//! Nothing here reads a clock. [`PointerBackend::translate`] is handed the
//! caller's [`EventTime`], and [`TranslationState::set_now`] lets a caller on
//! the free-function surface advance the same field. A state whose time never
//! advances simply never opens a suppression window — which is exactly the
//! behaviour an app that has not yet wired touch wants.
//!
//! # The kill switch
//!
//! [`InputTokens::touch_enabled`] is honoured **here**, at the first point a
//! finger becomes a Teksilo concept. With it off, a touch packet yields no
//! sample at all: no id is minted, no contact is tracked, no suppressor arms.
//! That is the programme's rollback switch, and it has to sit at the producer
//! for the rollback to be total.
//!
//! Reference: `docs/touch-and-pen.md`.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use teksilo_canvas::Point;
use teksilo_core::event::{ButtonMask, Key, Modifiers, PointerButton, ScrollDelta, WidgetEvent};
use teksilo_core::gesture::{GestureEvent, TapEvent};
use teksilo_core::pointer::{
    BackendDeviceKey, EventTime, PointerId, PointerIdAllocator, PointerInfo, PointerPhase,
    PointerSample, ScrollPhase, ScrollSample, ScrollSource,
};
use teksilo_core::trace_input;
use teksilo_tokens::{InputTokens, PenKind, PointerKind};

use crate::pen::{PenButtons, PenPacket, PenSource};
use crate::pointer_backend::{
    BackendCaps, BackendEvent, InputSample, PlatformKind, PointerBackend,
};
use crate::window_system::WindowSystem;

// ---------------------------------------------------------------------------
// Tuning constants
// ---------------------------------------------------------------------------

/// How near a lifted contact's position an emulated `CursorMoved` has to be to
/// count as the ghost the X11 core pointer leaves behind, in logical pixels.
///
/// One pixel: the core pointer is *warped* to the contact, so the ghost is at
/// the lift point exactly. Anything further away is a real mouse the user is
/// moving, and gets through.
const PHANTOM_SLOP: f32 = 1.0;

/// How long after the last lift the X11 core pointer's parked position is still
/// treated as a ghost.
const PHANTOM_LIFT_WINDOW: Duration = Duration::from_millis(150);

/// How long after the last lift a *button* event is treated as an emulated
/// click on a platform that promotes touch to mouse.
///
/// Longer than [`PHANTOM_LIFT_WINDOW`] because a promoted click is emitted
/// after the whole tap gesture has been recognised by the OS, not during it.
const PROMOTED_CLICK_WINDOW: Duration = Duration::from_millis(500);

/// How long after a scroll gesture's `Ended` a fresh `Started` is read as the
/// OS handing over its own momentum rather than as a new gesture.
///
/// This exists because winit 0.30's macOS backend **collapses** `NSEvent`'s
/// `phase` and `momentumPhase` into one `TouchPhase` (see
/// `platform_impl/macos/view.rs`, `scrollWheel:`): a momentum `Began` is
/// indistinguishable from a finger-down `Began` in the event alone. AppKit
/// hands momentum over in the same run-loop turn as the lift, so a short
/// window separates the two reliably. Without this, a two-finger flick reads
/// as two gestures and P12 would add a Teksilo fling on top of the OS's.
const MOMENTUM_HANDOFF_WINDOW: Duration = Duration::from_millis(100);

/// The device key every pen session is minted under.
///
/// A pen does not arrive through winit, so there is no `DeviceId` to hash. One
/// fixed key plus a process-global session counter is enough: the counter is
/// what makes two windows' sessions distinct, and the allocator only ever sees
/// `(PEN_DEVICE, session)` pairs that no window has used before.
const PEN_DEVICE: BackendDeviceKey = BackendDeviceKey::new(0x7065_6E5F_0000_0001);

/// The next pen proximity session id. Process-global, because
/// [`PointerIdAllocator`] is, and two windows with a stylus each must not mint
/// the same key.
static NEXT_PEN_SESSION: AtomicU64 = AtomicU64::new(1);

// ---------------------------------------------------------------------------
// Per-window state
// ---------------------------------------------------------------------------

/// One live touch contact.
#[derive(Copy, Clone, Debug)]
struct Contact {
    /// The identity minted for this press.
    id: PointerId,
    /// Where it was last seen, in window-logical coordinates.
    position: Point,
    /// Whether it is the primary contact of its sequence — the first one down
    /// while no other was live. W3C `isPrimary`: once it lifts, no other
    /// contact is promoted; the next sequence elects a new one.
    primary: bool,
}

/// One pen proximity session.
///
/// A session begins when the tool comes into range and ends when it leaves;
/// the tip touching and lifting inside that span are *button* transitions on
/// one pointer, not two pointers. That is the W3C model, and it is what makes
/// a hovering stylus drive tooltips and hover visuals the way a mouse does.
#[derive(Copy, Clone, Debug)]
struct PenContact {
    /// The identity minted for this proximity session.
    id: PointerId,
    /// The allocator key this session was minted under.
    session: u64,
    /// The tool in use. A tool change is a new session, not a mutation: a pen
    /// flipped to its eraser is a different pointer as far as a drawing
    /// surface is concerned.
    tool: PenKind,
    /// Last reported position, in window-logical coordinates.
    position: Point,
    /// Whether the tip is in contact.
    down: bool,
    /// The stylus buttons held.
    buttons: PenButtons,
    /// Whether this pointer is the primary one — see
    /// [`TranslationState::begin_pen_session`].
    primary: bool,
}

/// Where a wheel/trackpad stream currently sits.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
enum ScrollStreamState {
    /// No gesture in progress. A `Moved` here is a discrete wheel notch, which
    /// is what every scroll in Teksilo was before the touch programme.
    #[default]
    Idle,
    /// Fingers are down and moving.
    InGesture,
    /// The fingers lifted and the OS is coasting the content.
    InMomentum,
}

/// State tracked during event translation, one per window.
#[derive(Debug)]
pub struct TranslationState {
    scale_factor: f64,
    cursor_position: Option<Point>,
    current_modifiers: Modifiers,

    /// Which window system this window actually runs on. Set by the caller
    /// from `window_system_for_display_handle`; `Unknown` — the default — is
    /// precisely the set {Windows, macOS, headless}, none of which promote
    /// touch to mouse, so it is a safe default for the suppressors.
    window_system: WindowSystem,

    /// The input tokens in force. Carries the `touch_enabled` kill switch and
    /// `lines_per_notch`. Defaults to [`InputTokens::default`], whose
    /// `lines_per_notch` is 3.0 — the constant this module used to hardcode.
    input: InputTokens,

    /// The caller's notion of now. See the module docs.
    now: EventTime,

    /// Live contacts, keyed the way [`PointerIdAllocator`] keys them.
    contacts: HashMap<(BackendDeviceKey, u64), Contact>,
    /// Where and when the most recent contact lifted, for the two suppression
    /// windows.
    last_lift: Option<(Point, EventTime)>,

    /// Mouse buttons currently held, in press order. A `Vec` rather than a
    /// bitmask because [`ButtonMask`] is a union/intersection type with no
    /// "remove"; five entries is the ceiling.
    mouse_buttons: Vec<PointerButton>,

    /// The scroll-phase machine.
    scroll_state: ScrollStreamState,
    /// When the last gesture `Ended`, for the momentum handoff.
    scroll_ended_at: Option<EventTime>,

    /// Whether the "dropped a press with no cursor position" note has been
    /// traced. Once per window is enough to diagnose it; per packet would be a
    /// flood.
    warned_press_without_cursor: bool,

    /// The window's pen shim, if it has one. `None` on a platform with no pen
    /// path, and on a window nobody has attached one to.
    pen: Option<Box<dyn PenSource>>,
    /// The live pen proximity session.
    pen_contact: Option<PenContact>,
    /// Reused packet buffer, so polling a pen allocates nothing per turn.
    pen_scratch: Vec<PenPacket>,
}

impl TranslationState {
    /// A fresh per-window state: scale 1.0, no cursor, no modifiers, no
    /// contacts, default input tokens, `WindowSystem::Unknown`.
    pub fn new() -> Self {
        Self {
            scale_factor: 1.0,
            cursor_position: None,
            current_modifiers: Modifiers::NONE,
            window_system: WindowSystem::Unknown,
            input: InputTokens::default(),
            now: EventTime::ZERO,
            contacts: HashMap::new(),
            last_lift: None,
            mouse_buttons: Vec::new(),
            scroll_state: ScrollStreamState::default(),
            scroll_ended_at: None,
            warned_press_without_cursor: false,
            pen: None,
            pen_contact: None,
            pen_scratch: Vec::new(),
        }
    }

    pub fn set_scale_factor(&mut self, factor: f64) {
        self.scale_factor = factor;
    }

    pub fn scale_factor(&self) -> f64 {
        self.scale_factor
    }

    pub fn cursor_position(&self) -> Option<Point> {
        self.cursor_position
    }

    pub fn set_modifiers(&mut self, modifiers: Modifiers) {
        self.current_modifiers = modifiers;
    }

    /// The modifiers last reported by the OS.
    pub fn modifiers(&self) -> Modifiers {
        self.current_modifiers
    }

    /// Tell the translator which window system this window runs on.
    ///
    /// Read it from the live window with
    /// [`window_system_for_display_handle`](crate::window_system::window_system_for_display_handle)
    /// — never from the environment, which lies in a Wayland session running
    /// an X11 client.
    ///
    /// This selects the touch/mouse dual-stream suppressors; see
    /// [`BackendCaps::synthesises_mouse_from_touch`].
    pub fn set_window_system(&mut self, window_system: WindowSystem) {
        self.window_system = window_system;
    }

    /// The window system this state is translating for.
    pub fn window_system(&self) -> WindowSystem {
        self.window_system
    }

    /// Install the input tokens in force.
    ///
    /// The translator deliberately holds an [`InputTokens`] rather than a
    /// `Theme`: `teksilo-platform` already depends on `teksilo-tokens`, so this
    /// costs no new dependency and no dependency inversion (the token crate is
    /// a leaf and knows nothing of the widget tree). The caller re-installs
    /// them whenever the theme changes.
    pub fn set_input_tokens(&mut self, input: InputTokens) {
        self.input = input;
    }

    /// The input tokens in force.
    pub fn input_tokens(&self) -> &InputTokens {
        &self.input
    }

    /// Advance the translator's notion of now.
    ///
    /// [`PointerBackend::translate`] does this itself from its `now` argument;
    /// a caller still on the free-function surface calls it once per event
    /// batch so the suppression windows are measured against the tree's clock
    /// rather than against nothing.
    pub fn set_now(&mut self, now: EventTime) {
        if now > self.now {
            self.now = now;
        }
    }

    /// The translator's notion of now.
    pub fn now(&self) -> EventTime {
        self.now
    }

    /// How many contacts are currently down.
    pub fn live_contact_count(&self) -> usize {
        self.contacts.len()
    }

    /// Whether this window's platform also synthesises a mouse stream from
    /// touch, so that one of the two must be suppressed.
    fn promotes_touch_to_mouse(&self) -> bool {
        BackendCaps::for_platform(PlatformKind::HOST, self.window_system)
            .synthesises_mouse_from_touch
    }

    /// Whether a `CursorMoved` at `position` is the emulated pointer following
    /// a finger rather than a mouse the user is moving.
    ///
    /// **While a contact is live, every `CursorMoved` is dropped.** X11 warps
    /// the virtual core pointer onto the first concurrently-active contact and
    /// reports it through the *same* virtual device a real mouse uses
    /// (`util::VIRTUAL_CORE_POINTER`), so the two are indistinguishable at this
    /// layer — winit filters emulated *buttons* by `XIPointerEmulated` but
    /// emits this motion itself, deliberately, on every phase of the first
    /// contact. A rule that only dropped moves *within a pixel* of a contact
    /// would let every sample of a moving finger through.
    ///
    /// After the lift the core pointer stays parked at the lift point, so the
    /// narrow proximity rule takes over: a move still at that point is the
    /// ghost, a move anywhere else is a real mouse and gets through at once.
    ///
    /// # Residual
    ///
    /// winit emits its synthetic `CursorMoved` **before** the `Touch` packet
    /// that establishes the contact, so the very first move of a touch session
    /// that follows more than [`PHANTOM_LIFT_WINDOW`] of quiet still leaks one
    /// sample. Closing it would need one event of lookahead, which would cost
    /// every real X11 mouse move a frame of latency. Documented rather than
    /// paid for.
    fn is_phantom_motion(&self, position: Point) -> bool {
        if !self.promotes_touch_to_mouse() {
            return false;
        }
        if !self.contacts.is_empty() {
            return true;
        }
        match self.last_lift {
            Some((lift, at)) if self.now.saturating_since(at) <= PHANTOM_LIFT_WINDOW => {
                near(lift, position, PHANTOM_SLOP)
            }
            _ => false,
        }
    }

    /// Whether a mouse button event is the OS's promoted click for a tap that
    /// already reached the tree as touch.
    ///
    /// Defence in depth: winit 0.30 already drops X11's `XIPointerEmulated`
    /// button events, so on today's backends this window never fires. It is
    /// here because "the OS also sends a click" is the single most common way
    /// a touch port double-fires, and because a backend that does *not* filter
    /// (Android, Web, a future X11 rework) must not be able to introduce it
    /// silently.
    fn is_promoted_click(&self) -> bool {
        if !self.promotes_touch_to_mouse() {
            return false;
        }
        if !self.contacts.is_empty() {
            return true;
        }
        matches!(
            self.last_lift,
            Some((_, at)) if self.now.saturating_since(at) <= PROMOTED_CLICK_WINDOW
        )
    }

    /// The mouse's pointer identity as of now.
    fn mouse_pointer(&self) -> PointerInfo {
        let mut info = PointerInfo::mouse(self.now);
        info.buttons = self
            .mouse_buttons
            .iter()
            .fold(ButtonMask::NONE, |mask, b| mask.union((*b).into()));
        info
    }

    /// Translate one winit `Touch` packet.
    ///
    /// Returns `None` — with nothing recorded and no id minted — when touch is
    /// disabled, or when the packet belongs to a contact this window never saw
    /// go down (a stream that began before the window was listening, or before
    /// the kill switch was flipped on). Emitting an `Up` for a `Down` that
    /// never happened would break the cancel-completeness invariant just as
    /// surely as dropping one.
    fn translate_touch(&mut self, touch: &winit::event::Touch) -> Option<PointerSample> {
        if !self.input.touch_enabled {
            trace_input!(
                Samples,
                "touch dropped: touch_enabled=false (os id {})",
                touch.id
            );
            return None;
        }

        let device = device_key(touch.device_id);
        let key = (device, touch.id);
        let position = Point::new(
            (touch.location.x / self.scale_factor) as f32,
            (touch.location.y / self.scale_factor) as f32,
        );

        let (phase, id, primary) = match touch.phase {
            winit::event::TouchPhase::Started => {
                // A fresh identity per press. winit reuses `Touch::id` after a
                // lift, and a table keyed on the raw id would hand the new
                // contact the old one's gesture state.
                let id = PointerIdAllocator::global().begin(device, touch.id);
                let primary = self.contacts.is_empty();
                self.contacts.insert(
                    key,
                    Contact {
                        id,
                        position,
                        primary,
                    },
                );
                (PointerPhase::Down, id, primary)
            }
            winit::event::TouchPhase::Moved => {
                let contact = self.contacts.get_mut(&key)?;
                contact.position = position;
                (PointerPhase::Move, contact.id, contact.primary)
            }
            winit::event::TouchPhase::Ended | winit::event::TouchPhase::Cancelled => {
                let contact = self.contacts.remove(&key)?;
                PointerIdAllocator::global().end(device, touch.id);
                self.last_lift = Some((position, self.now));
                let phase = if matches!(touch.phase, winit::event::TouchPhase::Ended) {
                    PointerPhase::Up
                } else {
                    PointerPhase::Cancel
                };
                (phase, contact.id, contact.primary)
            }
        };

        let mut pointer = PointerInfo::touch(id, self.now);
        pointer.primary = primary;
        // A finger holds the primary "button" for as long as it is down. This
        // is normative, not cosmetic: every `accept_buttons()` recognizer in
        // the framework gates on `ButtonMask::PRIMARY`, so a contact that
        // reported an empty mask would be invisible to tap, drag, long-press
        // and multi-tap alike.
        pointer.buttons = match phase {
            PointerPhase::Down | PointerPhase::Move => ButtonMask::PRIMARY,
            PointerPhase::Up | PointerPhase::Cancel => ButtonMask::NONE,
        };
        pointer.axes.pressure = touch.force.and_then(pressure_from_force);
        // The contact patch, where a shim can supply one winit cannot. Windows
        // is the only platform that reports it today: `POINTER_TOUCH_INFO`
        // carries `rcContact` and `WM_TOUCH` — the path winit 0.30 takes —
        // does not.
        pointer.axes.contact = self
            .pen
            .as_ref()
            .and_then(|source| source.touch_contact(touch.id));

        // A direct pointer reports the button that changed on the two phases
        // that change one. A move never does, and a cancel has no meaningful
        // end state at all.
        let button = match phase {
            PointerPhase::Down | PointerPhase::Up => Some(PointerButton::Primary),
            _ => None,
        };

        trace_input!(
            Samples,
            "touch {:?} {:?} os_id={} at {:?}",
            phase,
            id,
            touch.id,
            position
        );

        Some(PointerSample {
            pointer,
            phase,
            position,
            button,
            modifiers: self.current_modifiers,
            coalesced: Vec::new(),
        })
    }

    // -----------------------------------------------------------------
    // Pen
    // -----------------------------------------------------------------

    /// Install this window's pen shim.
    ///
    /// Build one with [`create_pen_source`](crate::pen::create_pen_source),
    /// which answers [`NullPenSource`](crate::pen::null::NullPenSource) where
    /// the platform has no pen path. A window with no source simply never
    /// produces a pen sample — which is also the pen's rollback switch, since
    /// `InputTokens::touch_enabled` deliberately does **not** gate it: a
    /// stylus is not a finger, and rolling touch back must not take the pen
    /// with it.
    pub fn set_pen_source(&mut self, source: Box<dyn PenSource>) {
        self.pen = Some(source);
    }

    /// Remove and return this window's pen shim.
    ///
    /// Any live proximity session is *not* terminated here — call
    /// [`cancel_all`](PointerBackend::cancel_all) first if the pointer has to
    /// be ended cleanly.
    pub fn take_pen_source(&mut self) -> Option<Box<dyn PenSource>> {
        self.pen.take()
    }

    /// Whether a pen shim is installed.
    pub fn has_pen_source(&self) -> bool {
        self.pen.is_some()
    }

    /// Whether a tool is currently in proximity — i.e. whether a pen is
    /// hovering or drawing right now.
    pub fn pen_in_proximity(&self) -> bool {
        self.pen_contact.is_some()
    }

    /// Drain the pen shim and translate everything it buffered.
    ///
    /// Call once per event-loop turn, alongside the winit events. Cheap and
    /// allocation-free when no stylus is in use: the shim returns nothing and
    /// the packet buffer is reused.
    pub fn poll_pen(&mut self, now: EventTime) -> Vec<InputSample> {
        self.set_now(now);
        // Take the source out so the translation below can borrow `self`
        // mutably; it goes straight back.
        let Some(mut source) = self.pen.take() else {
            return Vec::new();
        };
        let mut packets = std::mem::take(&mut self.pen_scratch);
        packets.clear();
        source.poll(&mut packets);
        self.pen = Some(source);

        let mut samples = Vec::new();
        for packet in &packets {
            samples.append(&mut self.translate_pen_packet(packet));
        }
        packets.clear();
        self.pen_scratch = packets;
        samples
    }

    /// Turn one digitizer packet into the samples its transitions imply.
    ///
    /// Public so a replay backend, or a platform shim Teksilo has not met, can
    /// feed the same state machine without reimplementing it.
    ///
    /// # The state machine
    ///
    /// A packet is a *level*; the transitions are derived by comparing it with
    /// the session's previous state.
    ///
    /// | transition | sample |
    /// | --- | --- |
    /// | out of range → in range | `Move` (a hover: no buttons, `down` false) |
    /// | position changed | `Move` |
    /// | tip touched down | `Down` with [`PointerButton::Primary`] |
    /// | tip lifted | `Up` with `Primary` |
    /// | barrel pressed / released | `Down` / `Up` with `Secondary` |
    /// | second barrel | `Down` / `Up` with `Middle` |
    /// | in range → out of range | `Cancel` |
    /// | tool changed mid-session | `Cancel`, then a fresh session |
    ///
    /// Within one packet the `Move` is emitted **first**, so a press always
    /// lands at a position the consumer has already seen.
    ///
    /// # Why leaving proximity is a `Cancel`
    ///
    /// [`PointerPhase`] has no *leave*, and a tool going out of range
    /// completes nothing: the completion, if there was one, was the tip's
    /// `Up`, which has already been delivered. `Cancel` is the phase that says
    /// "this pointer's life ended without completing an interaction", which is
    /// exactly what happened — and it is the right thing for the down case
    /// too, where a stylus yanked off the tablet mid-stroke must not read as a
    /// deliberate lift.
    pub fn translate_pen_packet(&mut self, packet: &PenPacket) -> Vec<InputSample> {
        let time = if packet.time == EventTime::ZERO {
            self.now
        } else {
            packet.time
        };
        let mut samples = Vec::new();

        // End the session first when the tool left range, or when the tool
        // itself changed under us (pen → eraser is a different pointer).
        if let Some(contact) = self.pen_contact
            && (!packet.in_proximity || contact.tool != packet.tool)
        {
            samples.push(self.end_pen_session(contact, packet.position, time));
        }
        if !packet.in_proximity {
            return samples;
        }

        let (mut state, just_entered) = match self.pen_contact {
            Some(contact) => (contact, false),
            None => {
                let contact = self.begin_pen_session(packet);
                // The hover enter: a `Move` with nothing held, at the position
                // the tool came into range at.
                samples.push(self.pen_sample(&contact, PointerPhase::Move, None, packet, time));
                (contact, true)
            }
        };

        // Which buttons changed, in a fixed order: the tip first, then the
        // barrel, so a press-while-moving reads the same way every time.
        let mut transitions: Vec<(PointerPhase, PointerButton)> = Vec::new();
        if state.down != packet.down {
            transitions.push((
                if packet.down {
                    PointerPhase::Down
                } else {
                    PointerPhase::Up
                },
                PointerButton::Primary,
            ));
        }
        for (bit, button) in [
            (PenButtons::BARREL, PointerButton::Secondary),
            (PenButtons::SECONDARY_BARREL, PointerButton::Middle),
        ] {
            let was = state.buttons.contains(bit);
            let held = packet.buttons.contains(bit);
            if was != held {
                transitions.push((
                    if held {
                        PointerPhase::Down
                    } else {
                        PointerPhase::Up
                    },
                    button,
                ));
            }
        }

        let moved = state.position != packet.position;
        state.position = packet.position;
        // A packet with no transition still says something — a pressure ramp,
        // a tilt change — so it becomes a `Move` even when the position stood
        // still. The entering packet is the exception: its `Move` has already
        // been emitted above, and repeating it would double every hover.
        if !just_entered && (moved || transitions.is_empty()) {
            samples.push(self.pen_sample(&state, PointerPhase::Move, None, packet, time));
        }
        for (phase, button) in transitions {
            let pressed = phase == PointerPhase::Down;
            match button {
                PointerButton::Primary => state.down = pressed,
                PointerButton::Secondary => {
                    state.buttons = state.buttons.with(PenButtons::BARREL, pressed);
                }
                _ => {
                    state.buttons = state.buttons.with(PenButtons::SECONDARY_BARREL, pressed);
                }
            }
            samples.push(self.pen_sample(&state, phase, Some(button), packet, time));
        }

        // Carry the packet's raw flags (the eraser bit among them) forward, so
        // the next comparison is against what the device actually said.
        state.buttons = packet.buttons;
        state.down = packet.down;
        self.pen_contact = Some(state);
        samples
    }

    /// Mint an identity for a tool that just came into range.
    ///
    /// Primacy: a pen with no finger on the glass is the primary direct
    /// pointer. A pen that arrives while contacts are live is not — the
    /// cross-kind arbitration (pen versus mouse) belongs to the tree's pointer
    /// table, which can see every live pointer; this only avoids claiming
    /// primacy the platform layer can already tell is taken.
    fn begin_pen_session(&mut self, packet: &PenPacket) -> PenContact {
        let session = NEXT_PEN_SESSION.fetch_add(1, Ordering::Relaxed);
        let id = PointerIdAllocator::global().begin(PEN_DEVICE, session);
        let contact = PenContact {
            id,
            session,
            tool: packet.tool,
            position: packet.position,
            down: false,
            buttons: PenButtons::NONE,
            primary: self.contacts.is_empty(),
        };
        trace_input!(
            Samples,
            "pen {:?} in proximity ({:?}) at {:?}",
            id,
            packet.tool,
            packet.position
        );
        self.pen_contact = Some(contact);
        contact
    }

    /// End a proximity session and release its identity.
    fn end_pen_session(
        &mut self,
        contact: PenContact,
        position: Point,
        time: EventTime,
    ) -> InputSample {
        PointerIdAllocator::global().end(PEN_DEVICE, contact.session);
        self.pen_contact = None;
        trace_input!(Samples, "pen {:?} left proximity", contact.id);

        let mut pointer = PointerInfo::touch(contact.id, time);
        pointer.kind = PointerKind::Pen(contact.tool);
        pointer.primary = contact.primary;
        pointer.buttons = ButtonMask::NONE;
        InputSample::Pointer(PointerSample {
            pointer,
            phase: PointerPhase::Cancel,
            position,
            button: None,
            modifiers: self.current_modifiers,
            coalesced: Vec::new(),
        })
    }

    /// One sample for the session's current state.
    fn pen_sample(
        &self,
        contact: &PenContact,
        phase: PointerPhase,
        button: Option<PointerButton>,
        packet: &PenPacket,
        time: EventTime,
    ) -> InputSample {
        let mut pointer = PointerInfo::touch(contact.id, time);
        pointer.kind = PointerKind::Pen(contact.tool);
        pointer.primary = contact.primary;
        pointer.buttons = pen_button_mask(contact.down, contact.buttons);
        pointer.axes.pressure = Some(packet.pressure.clamp(0.0, 1.0));
        pointer.axes.tilt = packet.tilt;
        pointer.axes.twist = packet.twist;
        InputSample::Pointer(PointerSample {
            pointer,
            phase,
            position: contact.position,
            button,
            modifiers: self.current_modifiers,
            coalesced: Vec::new(),
        })
    }

    /// Advance the scroll-phase machine and translate one `MouseWheel` packet.
    fn translate_scroll(
        &mut self,
        delta: winit::event::MouseScrollDelta,
        winit_phase: winit::event::TouchPhase,
    ) -> ScrollSample {
        use winit::event::TouchPhase;

        let in_handoff = matches!(
            self.scroll_ended_at,
            Some(at) if self.now.saturating_since(at) <= MOMENTUM_HANDOFF_WINDOW
        );

        let phase = match (winit_phase, self.scroll_state) {
            // A `Started` right after an `Ended` is the OS handing over its own
            // momentum, not a second gesture. See MOMENTUM_HANDOFF_WINDOW.
            (TouchPhase::Started, _) if in_handoff => {
                self.scroll_state = ScrollStreamState::InMomentum;
                ScrollPhase::Momentum
            }
            (TouchPhase::Started, _) => {
                self.scroll_state = ScrollStreamState::InGesture;
                self.scroll_ended_at = None;
                ScrollPhase::Began
            }
            (TouchPhase::Moved, ScrollStreamState::InGesture) => ScrollPhase::Changed,
            (TouchPhase::Moved, ScrollStreamState::InMomentum) => ScrollPhase::Momentum,
            // A wheel notch: winit reports `Moved` with no `Started` before it
            // on Windows and X11, and for a non-precise wheel on macOS. This is
            // the arm every mouse in the world takes, and it is `Discrete` —
            // exactly what a scroll was before the touch programme.
            (TouchPhase::Moved, ScrollStreamState::Idle) => ScrollPhase::Discrete,
            (TouchPhase::Ended, ScrollStreamState::InGesture) => {
                self.scroll_state = ScrollStreamState::Idle;
                self.scroll_ended_at = Some(self.now);
                ScrollPhase::Ended
            }
            (TouchPhase::Ended, ScrollStreamState::InMomentum) => {
                self.scroll_state = ScrollStreamState::Idle;
                self.scroll_ended_at = None;
                ScrollPhase::MomentumEnded
            }
            // An `Ended` with nothing open. macOS emits one for a two-finger
            // rest that never became a scroll (`NSEventPhase::MayBegin` then
            // `Cancelled`). Reporting `Ended` would leave a consumer with an
            // end it never saw a beginning for, so it degrades to a notch.
            (TouchPhase::Ended, ScrollStreamState::Idle) => ScrollPhase::Discrete,
            (TouchPhase::Cancelled, _) => {
                self.scroll_state = ScrollStreamState::Idle;
                self.scroll_ended_at = None;
                ScrollPhase::Cancelled
            }
        };

        let (scroll_delta, source) = self.scroll_delta(delta);
        trace_input!(
            Samples,
            "scroll {:?} {:?}/{:?}",
            scroll_delta,
            phase,
            source
        );

        ScrollSample {
            delta: scroll_delta,
            // `None` routes by hover, which is what every scroll in Teksilo
            // has always done and what an *indirect* pointer wants: a mouse
            // and a trackpad both move the cursor, so the hovered widget is
            // by construction the one under the gesture. Only a direct
            // contact — a synthesised touch pan, which lands with the
            // kinetic-scrolling package — needs positional routing, because a
            // finger never writes hover.
            position: None,
            phase,
            source,
            pointer: self.mouse_pointer(),
            modifiers: self.current_modifiers,
        }
    }

    /// The signed delta and the source a winit scroll delta implies.
    fn scroll_delta(&self, delta: winit::event::MouseScrollDelta) -> (ScrollDelta, ScrollSource) {
        match delta {
            winit::event::MouseScrollDelta::LineDelta(x, y) => (
                ScrollDelta::Lines {
                    x: -x * self.input.lines_per_notch,
                    y: -y * self.input.lines_per_notch,
                },
                ScrollSource::Wheel,
            ),
            // winit hands over pixel deltas only where the device reports
            // precise scrolling (macOS `hasPreciseScrollingDeltas`, a Wayland
            // `axis` in surface-local units), which is a trackpad or a
            // free-spinning wheel.
            winit::event::MouseScrollDelta::PixelDelta(pos) => (
                ScrollDelta::Pixels {
                    x: -(pos.x / self.scale_factor) as f32,
                    y: -(pos.y / self.scale_factor) as f32,
                },
                ScrollSource::Trackpad,
            ),
        }
    }
}

impl Default for TranslationState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// The backend impl
// ---------------------------------------------------------------------------

impl PointerBackend for TranslationState {
    fn translate(&mut self, event: &BackendEvent<'_>, now: EventTime) -> Vec<InputSample> {
        self.set_now(now);
        let BackendEvent::Winit(event) = *event;

        use winit::event::WindowEvent as WE;
        match event {
            WE::CursorMoved { position, .. } => {
                let logical = Point::new(
                    (position.x / self.scale_factor) as f32,
                    (position.y / self.scale_factor) as f32,
                );
                if self.is_phantom_motion(logical) {
                    trace_input!(
                        Samples,
                        "cursor move suppressed (emulated) at {:?}",
                        logical
                    );
                    return Vec::new();
                }
                self.cursor_position = Some(logical);
                let mut sample = PointerSample::mouse(PointerPhase::Move, logical, self.now);
                sample.pointer = self.mouse_pointer();
                sample.modifiers = self.current_modifiers;
                vec![InputSample::Pointer(sample)]
            }

            WE::MouseInput { state, button, .. } => {
                let Some(button) = translate_mouse_button(*button) else {
                    return Vec::new();
                };
                let Some(position) = self.cursor_position else {
                    self.note_press_without_cursor();
                    return Vec::new();
                };
                if self.is_promoted_click() {
                    trace_input!(
                        Samples,
                        "mouse {:?} suppressed (promoted from touch)",
                        button
                    );
                    return Vec::new();
                }
                let phase = match state {
                    winit::event::ElementState::Pressed => {
                        if !self.mouse_buttons.contains(&button) {
                            self.mouse_buttons.push(button);
                        }
                        PointerPhase::Down
                    }
                    winit::event::ElementState::Released => {
                        self.mouse_buttons.retain(|b| *b != button);
                        PointerPhase::Up
                    }
                };
                let mut sample = PointerSample::mouse(phase, position, self.now);
                sample.pointer = self.mouse_pointer();
                sample.button = Some(button);
                sample.modifiers = self.current_modifiers;
                vec![InputSample::Pointer(sample)]
            }

            WE::MouseWheel { delta, phase, .. } => {
                vec![InputSample::Scroll(self.translate_scroll(*delta, *phase))]
            }

            WE::Touch(touch) => self
                .translate_touch(touch)
                .map(InputSample::Pointer)
                .into_iter()
                .collect(),

            WE::PinchGesture { delta, phase, .. } => {
                vec![InputSample::Gesture(pinch_gesture(
                    *delta,
                    *phase,
                    self.cursor_position.unwrap_or(Point::ZERO),
                ))]
            }

            WE::RotationGesture { delta, phase, .. } => {
                vec![InputSample::Gesture(rotation_gesture(
                    *delta,
                    *phase,
                    self.cursor_position.unwrap_or(Point::ZERO),
                ))]
            }

            WE::DoubleTapGesture { .. } => vec![InputSample::Gesture(double_tap_gesture(
                self.cursor_position.unwrap_or(Point::ZERO),
                self.current_modifiers,
            ))],

            _ => Vec::new(),
        }
    }

    fn capabilities(&self) -> BackendCaps {
        let mut caps = BackendCaps::for_platform(PlatformKind::HOST, self.window_system);
        // A pen shim adds what winit cannot report; it never takes anything
        // away. On a window with no shim this is a no-op and the row is
        // exactly the platform's.
        if let Some(pen) = &self.pen {
            pen.capabilities().apply_to(&mut caps);
        }
        caps
    }

    fn cancel_all(&mut self, now: EventTime) -> Vec<InputSample> {
        self.set_now(now);

        // Drain in mint order so the oldest contact is cancelled first — the
        // order a multi-touch consumer's own bookkeeping is in.
        let mut contacts: Vec<((BackendDeviceKey, u64), Contact)> = self.contacts.drain().collect();
        contacts.sort_by_key(|(_, contact)| contact.id);

        let mut samples: Vec<InputSample> = Vec::with_capacity(contacts.len() + 1);
        for ((device, os_id), contact) in contacts {
            PointerIdAllocator::global().end(device, os_id);
            let mut pointer = PointerInfo::touch(contact.id, self.now);
            pointer.primary = contact.primary;
            pointer.buttons = ButtonMask::NONE;
            trace_input!(Samples, "cancel_all {:?}", contact.id);
            samples.push(InputSample::Pointer(PointerSample {
                pointer,
                phase: PointerPhase::Cancel,
                position: contact.position,
                button: None,
                modifiers: self.current_modifiers,
                coalesced: Vec::new(),
            }));
        }

        // A pen in proximity is a live pointer too, held or not: the window
        // that is losing the stream is the one that was hovering.
        if let Some(contact) = self.pen_contact {
            let position = contact.position;
            samples.push(self.end_pen_session(contact, position, self.now));
        }

        // A held mouse button is a live pointer too: a window that loses the
        // stream mid-drag must not leave the press unterminated either.
        if !self.mouse_buttons.is_empty()
            && let Some(position) = self.cursor_position
        {
            self.mouse_buttons.clear();
            let mut sample = PointerSample::mouse(PointerPhase::Cancel, position, self.now);
            sample.modifiers = self.current_modifiers;
            samples.push(InputSample::Pointer(sample));
        }

        samples
    }
}

impl TranslationState {
    /// Trace the dropped press once per window.
    fn note_press_without_cursor(&mut self) {
        if !self.warned_press_without_cursor {
            self.warned_press_without_cursor = true;
            trace_input!(
                Samples,
                "mouse button dropped: no cursor position yet (was dispatched at the \
                 window origin before P15)"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Derive a stable per-device key from winit's opaque `DeviceId`.
///
/// `DeviceId` is `Hash + Eq` but its inner value is `pub(crate)`, so hashing is
/// the only way to get a number out of it. `DefaultHasher::new()` is seeded
/// with fixed keys (it is *not* `RandomState`), so the mapping is deterministic
/// within a run and reproducible across runs — which matters because the key
/// appears in trace output.
///
/// A hash collision between two devices would merge their contact id spaces.
/// With a 64-bit SipHash and a handful of devices the probability is not worth
/// a second field: the birthday bound for 100 devices is about 2.7e-16.
fn device_key(device_id: winit::event::DeviceId) -> BackendDeviceKey {
    let mut hasher = DefaultHasher::new();
    device_id.hash(&mut hasher);
    BackendDeviceKey::new(hasher.finish())
}

/// The button mask a pen holds: the tip is `Primary`, the barrel `Secondary`,
/// a second barrel `Middle`.
///
/// The tip mapping is normative rather than cosmetic — every
/// `accept_buttons()` recognizer in the framework gates on
/// `ButtonMask::PRIMARY`, so a stylus that reported anything else would be
/// invisible to tap, drag and long-press alike.
fn pen_button_mask(down: bool, buttons: PenButtons) -> ButtonMask {
    let mut mask = ButtonMask::NONE;
    if down {
        mask = mask.union(PointerButton::Primary.into());
    }
    if buttons.contains(PenButtons::BARREL) {
        mask = mask.union(PointerButton::Secondary.into());
    }
    if buttons.contains(PenButtons::SECONDARY_BARREL) {
        mask = mask.union(PointerButton::Middle.into());
    }
    mask
}

/// Whether two points are within `slop` logical pixels of each other.
fn near(a: Point, b: Point, slop: f32) -> bool {
    (a.x - b.x).abs() <= slop && (a.y - b.y).abs() <= slop
}

/// Normalised tip pressure from a winit `Force`, or `None` when the device's
/// numbers cannot produce one.
///
/// winit's own `Force::normalized()` divides by `sin(altitude_angle)` to
/// recover the component perpendicular to the surface, then by
/// `max_possible_force`. Both divisors can be zero — a stylus lying flat on the
/// glass has `altitude_angle == 0` — and the result is then infinite rather
/// than an error. Teksilo re-implements the conversion so those cases become
/// "no pressure reported" instead of an infinity in a `PointerAxes`.
fn pressure_from_force(force: winit::event::Force) -> Option<f32> {
    let normalized = match force {
        winit::event::Force::Normalized(value) => value,
        winit::event::Force::Calibrated {
            force,
            max_possible_force,
            altitude_angle,
        } => {
            if max_possible_force <= 0.0 {
                return None;
            }
            let perpendicular = match altitude_angle {
                Some(angle) => {
                    let sin = angle.sin();
                    if sin <= f64::EPSILON {
                        return None;
                    }
                    force / sin
                }
                None => force,
            };
            perpendicular / max_possible_force
        }
    };
    if !normalized.is_finite() {
        return None;
    }
    Some((normalized as f32).clamp(0.0, 1.0))
}

// ---------------------------------------------------------------------------
// The free-function surface
// ---------------------------------------------------------------------------

/// Translate a winit CursorMoved event to a WidgetEvent::PointerMove.
///
/// Returns `None` when the move is the emulated pointer following a finger —
/// see `TranslationState::is_phantom_motion`. On a platform that does not
/// promote touch to mouse (everything but X11) that check is a constant
/// `false` and the translation is unchanged.
pub fn translate_cursor_moved(
    physical_x: f64,
    physical_y: f64,
    state: &mut TranslationState,
) -> Option<WidgetEvent> {
    let logical_x = (physical_x / state.scale_factor) as f32;
    let logical_y = (physical_y / state.scale_factor) as f32;
    let position = Point::new(logical_x, logical_y);
    if state.is_phantom_motion(position) {
        return None;
    }
    state.cursor_position = Some(position);
    Some(WidgetEvent::PointerMove { position })
}

/// Translate a winit `Ime` event into a teksilo-core `WidgetEvent`.
///
/// - `Preedit(text, cursor)` → `ImeComposition`. The `cursor` byte indices
///   `(begin, end)` index into the preedit `text` and are preserved as a
///   `Range`. `None` (hide-cursor) and empty `text` (winit's synthetic
///   clear, emitted right before `Commit`) flow through faithfully.
/// - `Commit(text)` → `ImeCommit`.
/// - `Enabled` / `Disabled` are OS acknowledgements (enablement is driven
///   by the focused node's descriptor) and produce no tree event.
pub fn translate_ime(ime: winit::event::Ime) -> Option<WidgetEvent> {
    match ime {
        winit::event::Ime::Preedit(text, cursor) => Some(WidgetEvent::ImeComposition {
            text,
            cursor: cursor.map(|(begin, end)| begin..end),
        }),
        winit::event::Ime::Commit(text) => Some(WidgetEvent::ImeCommit { text }),
        winit::event::Ime::Enabled | winit::event::Ime::Disabled => None,
    }
}

/// Translate a winit mouse button to a teksilo-core PointerButton.
pub fn translate_mouse_button(button: winit::event::MouseButton) -> Option<PointerButton> {
    match button {
        winit::event::MouseButton::Left => Some(PointerButton::Primary),
        winit::event::MouseButton::Right => Some(PointerButton::Secondary),
        winit::event::MouseButton::Middle => Some(PointerButton::Middle),
        winit::event::MouseButton::Back => Some(PointerButton::Back),
        winit::event::MouseButton::Forward => Some(PointerButton::Forward),
        // MouseButton::Other(_) — vendor-specific extra buttons we don't
        // currently surface. Returning None drops the event.
        _ => None,
    }
}

/// Translate a winit ElementState + MouseButton to PointerDown/Up.
///
/// Returns `None` when no cursor position is known yet. This used to dispatch
/// the press at `Point::ZERO`, which is a click on whatever sits in the
/// window's top-left corner — a real misfire on every platform that can deliver
/// a button before a motion (X11 with a grab, a synthetic click, a window that
/// gains the pointer already pressed).
pub fn translate_mouse_input(
    button_state: winit::event::ElementState,
    button: winit::event::MouseButton,
    state: &TranslationState,
) -> Option<WidgetEvent> {
    let pointer_button = translate_mouse_button(button)?;
    let position = state.cursor_position?;
    if state.is_promoted_click() {
        return None;
    }
    match button_state {
        winit::event::ElementState::Pressed => Some(WidgetEvent::PointerDown {
            position,
            button: pointer_button,
            modifiers: state.current_modifiers,
        }),
        winit::event::ElementState::Released => Some(WidgetEvent::PointerUp {
            position,
            button: pointer_button,
            modifiers: state.current_modifiers,
        }),
    }
}

/// Translate winit keyboard modifiers to teksilo-core Modifiers.
pub fn translate_modifiers(mods: winit::keyboard::ModifiersState) -> Modifiers {
    let mut result = Modifiers::NONE;
    if mods.control_key() {
        result = result | Modifiers::CTRL;
    }
    if mods.shift_key() {
        result = result | Modifiers::SHIFT;
    }
    if mods.alt_key() {
        result = result | Modifiers::ALT;
    }
    if mods.super_key() {
        result = result | Modifiers::SUPER;
    }
    result
}

/// Translate a winit logical key to a teksilo-core Key.
pub fn translate_key(key: &winit::keyboard::Key) -> Option<Key> {
    match key {
        winit::keyboard::Key::Named(named) => translate_named_key(*named),
        winit::keyboard::Key::Character(c) => {
            let ch = c.chars().next()?;
            match ch.to_ascii_uppercase() {
                'A' => Some(Key::A),
                'B' => Some(Key::B),
                'C' => Some(Key::C),
                'D' => Some(Key::D),
                'E' => Some(Key::E),
                'F' => Some(Key::F),
                'G' => Some(Key::G),
                'H' => Some(Key::H),
                'I' => Some(Key::I),
                'J' => Some(Key::J),
                'K' => Some(Key::K),
                'L' => Some(Key::L),
                'M' => Some(Key::M),
                'N' => Some(Key::N),
                'O' => Some(Key::O),
                'P' => Some(Key::P),
                'Q' => Some(Key::Q),
                'R' => Some(Key::R),
                'S' => Some(Key::S),
                'T' => Some(Key::T),
                'U' => Some(Key::U),
                'V' => Some(Key::V),
                'W' => Some(Key::W),
                'X' => Some(Key::X),
                'Y' => Some(Key::Y),
                'Z' => Some(Key::Z),
                _ => Some(Key::Character(ch)),
            }
        }
        _ => None,
    }
}

fn translate_named_key(key: winit::keyboard::NamedKey) -> Option<Key> {
    use winit::keyboard::NamedKey;
    match key {
        NamedKey::Space => Some(Key::Space),
        NamedKey::Enter => Some(Key::Enter),
        NamedKey::Escape => Some(Key::Escape),
        NamedKey::Tab => Some(Key::Tab),
        NamedKey::Backspace => Some(Key::Backspace),
        NamedKey::Delete => Some(Key::Delete),
        NamedKey::Insert => Some(Key::Insert),
        NamedKey::ArrowUp => Some(Key::ArrowUp),
        NamedKey::ArrowDown => Some(Key::ArrowDown),
        NamedKey::ArrowLeft => Some(Key::ArrowLeft),
        NamedKey::ArrowRight => Some(Key::ArrowRight),
        NamedKey::Home => Some(Key::Home),
        NamedKey::End => Some(Key::End),
        NamedKey::PageUp => Some(Key::PageUp),
        NamedKey::PageDown => Some(Key::PageDown),
        NamedKey::F1 => Some(Key::F1),
        NamedKey::F2 => Some(Key::F2),
        NamedKey::F3 => Some(Key::F3),
        NamedKey::F4 => Some(Key::F4),
        NamedKey::F5 => Some(Key::F5),
        NamedKey::F6 => Some(Key::F6),
        NamedKey::F7 => Some(Key::F7),
        NamedKey::F8 => Some(Key::F8),
        NamedKey::F9 => Some(Key::F9),
        NamedKey::F10 => Some(Key::F10),
        NamedKey::F11 => Some(Key::F11),
        NamedKey::F12 => Some(Key::F12),
        NamedKey::F13 => Some(Key::F13),
        NamedKey::F14 => Some(Key::F14),
        NamedKey::F15 => Some(Key::F15),
        NamedKey::F16 => Some(Key::F16),
        NamedKey::F17 => Some(Key::F17),
        NamedKey::F18 => Some(Key::F18),
        NamedKey::F19 => Some(Key::F19),
        NamedKey::F20 => Some(Key::F20),
        NamedKey::F21 => Some(Key::F21),
        NamedKey::F22 => Some(Key::F22),
        NamedKey::F23 => Some(Key::F23),
        NamedKey::F24 => Some(Key::F24),
        // Caps Lock arrives as a discrete press/release. winit's
        // `ModifiersState` carries no lock state, so the window manager
        // tracks the active state itself on the key-down edge (drives
        // `WindowState::caps_lock` for the password-field warning).
        NamedKey::CapsLock => Some(Key::CapsLock),
        // Windows `VK_APPS`, X11/Wayland `keysyms::Menu`. macOS produces this
        // zero times, which is why the dispatcher also reserves a chord.
        NamedKey::ContextMenu => Some(Key::ContextMenu),
        _ => None,
    }
}

/// Translate a winit MouseWheel event to a WidgetEvent::Scroll.
///
/// The lines-per-notch factor comes from
/// [`InputTokens::lines_per_notch`](teksilo_tokens::InputTokens::lines_per_notch)
/// on the state's installed tokens; its default is 3.0, the Windows/GTK
/// default and the constant this function used to hardcode.
pub fn translate_mouse_wheel(
    delta: winit::event::MouseScrollDelta,
    _phase: winit::event::TouchPhase,
    state: &TranslationState,
) -> Option<WidgetEvent> {
    // Winit uses "natural" sign: positive y = scroll up (content moves down).
    // Teksilo's ScrollDelta uses positive y = increase scroll offset (content
    // moves up). `scroll_delta` negates both axes to match.
    let (scroll_delta, _) = state.scroll_delta(delta);
    Some(WidgetEvent::scroll(scroll_delta, state.current_modifiers))
}

// --- Desktop trackpad gesture passthrough ---
// On desktop, most gestures arrive as already-recognized events from the OS
// trackpad driver. These functions translate winit's high-level gesture events
// into Teksilo GestureEvents. They are reached two ways: as
// `WidgetEvent::Gesture` through the free functions below, and as
// `InputSample::Gesture` through `PointerBackend::translate`.

/// A winit PinchGesture as a Teksilo gesture.
fn pinch_gesture(delta: f64, phase: winit::event::TouchPhase, center: Point) -> GestureEvent {
    match phase {
        winit::event::TouchPhase::Started => GestureEvent::PinchStarted { center },
        winit::event::TouchPhase::Moved => GestureEvent::PinchChanged {
            center,
            scale: 1.0 + delta as f32,
            rotation: 0.0,
        },
        winit::event::TouchPhase::Ended | winit::event::TouchPhase::Cancelled => {
            GestureEvent::PinchEnded
        }
    }
}

/// A winit RotationGesture as a Teksilo gesture.
fn rotation_gesture(
    delta_degrees: f32,
    phase: winit::event::TouchPhase,
    center: Point,
) -> GestureEvent {
    match phase {
        winit::event::TouchPhase::Started => GestureEvent::PinchStarted { center },
        winit::event::TouchPhase::Moved => GestureEvent::PinchChanged {
            center,
            scale: 1.0,
            rotation: delta_degrees,
        },
        winit::event::TouchPhase::Ended | winit::event::TouchPhase::Cancelled => {
            GestureEvent::PinchEnded
        }
    }
}

/// A winit DoubleTapGesture as a Teksilo gesture.
fn double_tap_gesture(position: Point, modifiers: Modifiers) -> GestureEvent {
    GestureEvent::DoubleTap(TapEvent::new(position, PointerButton::Primary, modifiers))
}

/// Translate a winit PinchGesture into a Teksilo gesture event.
/// Returns PinchStarted on Started phase, PinchChanged on Changed, PinchEnded on Ended.
pub fn translate_pinch_gesture(
    delta: f64,
    phase: winit::event::TouchPhase,
    state: &TranslationState,
) -> Option<WidgetEvent> {
    let center = state.cursor_position.unwrap_or(Point::ZERO);
    Some(WidgetEvent::Gesture {
        gesture: pinch_gesture(delta, phase, center),
    })
}

/// Translate a winit RotationGesture into a PinchChanged with rotation.
/// Rotation gestures are folded into the pinch gesture model since they
/// typically co-occur with pinch on trackpads.
pub fn translate_rotation_gesture(
    delta_degrees: f32,
    phase: winit::event::TouchPhase,
    state: &TranslationState,
) -> Option<WidgetEvent> {
    let center = state.cursor_position.unwrap_or(Point::ZERO);
    Some(WidgetEvent::Gesture {
        gesture: rotation_gesture(delta_degrees, phase, center),
    })
}

/// Translate a winit DoubleTapGesture (trackpad smart magnification).
///
/// Synthetic OS-driven double-tap: there's no underlying mouse button
/// or modifier set the OS hands us, so we attribute it to
/// `PointerButton::Primary` with no modifiers. Apps that need richer
/// trackpad-gesture metadata should match on `WidgetEvent::Gesture`
/// directly rather than hooking `on_double_tap`.
pub fn translate_double_tap_gesture(state: &TranslationState) -> Option<WidgetEvent> {
    let position = state.cursor_position.unwrap_or(Point::ZERO);
    Some(WidgetEvent::Gesture {
        gesture: double_tap_gesture(position, state.current_modifiers),
    })
}

#[cfg(test)]
mod tests;
