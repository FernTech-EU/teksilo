// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! Telling a screen reader about each key, where it cannot see them itself.
//!
//! A screen reader has to know the key behind every change it hears about:
//! Orca decides whether a caret move is read as a character, a word or a line
//! from the last key it saw (`_presentTextAtNewCaretPosition` in
//! `orca/scripts/default.py`), echoes typed keys, and runs its own commands
//! (Orca+T, which says the time, and the rest) from keys the application must
//! then never see.
//!
//! On Linux under Wayland it cannot read the keyboard. libatspi 2.52 gives it
//! the *legacy* keyboard device there, which learns of a key only when the
//! application that received it reports it to the AT-SPI registry: a call to
//! `org.a11y.atspi.DeviceEventController.NotifyListenersSync` on the
//! accessibility bus, whose `bool` answer says whether a listener (Orca,
//! running one of its commands) took the key. GTK 3's bridge
//! (`spi_atk_bridge_key_listener` in at-spi2-core's `atk-adaptor/event.c`)
//! and Qt's (`QSpiApplicationAdaptor::eventFilter`) make that call for every
//! key they handle, and neither delivers a key a listener took.
//! `accesskit_unix` makes no such call, so before this module a Teksilo
//! window in a Wayland session left Orca deaf to every key: no key echo, no
//! caret speech, no Orca command.
//!
//! [`KeyReportGate`] is that call, placed where `teksilo-app` receives winit's
//! `KeyboardInput`, before anything acts on the key:
//!
//! - **When.** Only while an assistive technology is attached, the same
//!   condition under which `accesskit_unix` exposes the tree at all
//!   ([`PlatformWindow::accessibility_active`](crate::PlatformWindow::accessibility_active)).
//!   On an X11 session Orca reads the keyboard itself and registers no
//!   listener, so the registry answers at once that nobody took the key: the
//!   cost of what GTK and Qt already do there.
//! - **What.** Every physical press and release, a held key's repeats as
//!   presses (as GTK reports each autorepeat event). Not winit's *synthetic*
//!   events, which X11 makes up for keys already held when a window gains or
//!   loses focus: nobody pressed them, and they are delivered untouched.
//! - **The struct.** `(uiiiisb)`: press 0 / release 1, the X keysym, the X
//!   keycode (the evdev code + 8), the X modifier mask, a time in
//!   milliseconds, the text the key types, and whether it types it (the
//!   crate's `DeviceEvent`). winit reports no lock state, so Num Lock is learnt from
//!   the keys that show it (a keypad digit: on; a keypad motion: off; the Num
//!   Lock key toggles what is known) and carried as Mod2 on every key while
//!   known on; until a key has shown it, it is taken as off.
//! - **Consumed keys.** A press a listener took is not delivered, and neither
//!   is its release: the release always follows its press, whatever the
//!   registry says about the release itself, so the application never sees a
//!   key go down without coming up or the other way round.
//! - **Time.** A press waits for the registry's answer, as GTK waits. A
//!   release does not: whether it is delivered is already decided by its
//!   press, and the next press waits behind it anyway, since reports are sent
//!   one at a time, in order.
//! - **Releases are never lost.** The release of a key whose press was
//!   reported is reported too, whatever window it comes up in (a window whose
//!   adapter a reader has not reached yet included), and never turned away
//!   for lack of room: libatspi's legacy device sets the Orca modifier
//!   (Insert, Caps Lock) from the press and clears it from the release, and a
//!   lost release leaves every later key looking like an Orca command.
//!   Presses behind a registry that hangs are bounded (64) and dropped
//!   unsent once a second old, rather than replayed when it wakes.
//! - **No registry, or a slow one.** The wait is bounded (200 ms, between Qt's
//!   100 ms and GTK 3's 500 ms), and a press that runs out of it is
//!   delivered. Until that report is finally answered, the presses that follow
//!   are reported without waiting. So a registry that stops answering costs
//!   one 200 ms pause, however many keys follow; one that answers every
//!   report, but each more than 200 ms late, costs up to 200 ms on every
//!   press made after it has answered the one before. A missing bus or
//!   registry is answered at once, and asked again at most every few
//!   seconds. The D-Bus traffic runs on a thread of its own; the event loop
//!   only ever waits on a channel.
//! - **Secure fields.** While a secure field has focus (one that declares
//!   `ImePurpose::Password`, as every `PasswordField` does, revealed or not),
//!   a key that types text (a character, a dead key, Space) is reported with
//!   no text and the keysym `VoidSymbol`, press and release; every other key
//!   is reported in full. Its physical keycode and modifiers are still
//!   reported, because Orca matches its own commands on them, so a process
//!   listening to the registry can still tell which physical keys were
//!   pressed there. GTK 3.24 (`atk_key_event_from_gdk_event_key`) and Qt 6.8
//!   (`QSpiApplicationAdaptor::eventFilter`) report such keys in full,
//!   character included. Orca needs none of it there: it echoes nothing in a
//!   password field (`presentKeyboardEvent`) and obscures those keys in its
//!   own log.
//! - **Input methods.** A key an input method takes never reaches Teksilo as a
//!   `KeyboardInput`, on either Linux backend: on Wayland the compositor gives
//!   it to the input method before the application, and on X11 winit drops
//!   every event `XFilterEvent` claims. So those keys are not reported, as
//!   they are not by a GTK application on Wayland; the composition itself
//!   reaches the reader as text changes.
//!
//! On Windows and macOS the gate reports nothing and delivers every key: their
//! screen readers read the keyboard themselves.

use std::time::Instant;

use winit::event::ElementState;
use winit::keyboard::{Key, KeyLocation, ModifiersState, NamedKey, PhysicalKey};

#[cfg(all(unix, not(target_os = "macos")))]
mod atspi;
mod keysym;
#[cfg(test)]
mod tests;
#[cfg(all(unix, not(target_os = "macos")))]
mod worker;

/// Whether a key event is a press or a release, as the registry numbers them
/// (`ATSPI_KEY_PRESSED_EVENT`, `ATSPI_KEY_RELEASED_EVENT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeviceEventKind {
    /// A key went down, or repeated while held.
    Pressed = 0,
    /// A key came up.
    Released = 1,
}

/// One key event as the AT-SPI registry reads it: the `(uiiiisb)` struct of
/// `NotifyListenersSync` (Qt's `QSpiDeviceEvent`; the registry's own
/// introspection XML says `(uiuuisb)`, but its demarshaller reads this).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeviceEvent {
    /// Press or release.
    pub kind: DeviceEventKind,
    /// The X keysym of the key, at the shift level it was pressed at: `0x61`
    /// for `a`, `0x41` for Shift+`a`, `0xff54` for Down.
    pub keysym: u32,
    /// The X keycode: the Linux evdev code plus 8. Orca matches its own
    /// commands on this.
    pub hw_code: u32,
    /// The X modifier mask held with the key: Shift `1 << 0`, Caps Lock
    /// `1 << 1`, Control `1 << 2`, Alt (Mod1) `1 << 3`, Num Lock (Mod2)
    /// `1 << 4`, Super (Mod4) `1 << 6`.
    pub modifiers: u32,
    /// Milliseconds on a clock of the reporter's own.
    pub timestamp: u32,
    /// The text of the key when it has printable text, else empty; a reader
    /// names a key from its keysym when this is empty.
    pub event_string: String,
    /// Whether the key types its text: it has some, and no Control, Alt or
    /// Super is held.
    pub is_text: bool,
}

impl DeviceEvent {
    /// The event for `key`, with the keyboard in `keyboard`, Num Lock known
    /// to be on or not, at `timestamp`. When `withhold`, a key that types
    /// text (a secure field has focus) is reported with no text and
    /// `VoidSymbol`: its keycode and modifiers only.
    pub(crate) fn for_key(
        key: &KeyInput<'_>,
        keyboard: KeyboardState,
        num_lock: bool,
        withhold: bool,
        timestamp: u32,
    ) -> Self {
        let modifiers = keyboard.modifiers;
        let withheld = withhold && keysym::types_text(key.logical_key);
        let event_string = if withheld {
            String::new()
        } else {
            keysym::event_string(key.logical_key)
        };
        let chorded = modifiers.control_key() || modifiers.alt_key() || modifiers.super_key();
        Self {
            kind: match key.state {
                ElementState::Pressed => DeviceEventKind::Pressed,
                ElementState::Released => DeviceEventKind::Released,
            },
            keysym: if withheld {
                keysym::VOID_SYMBOL
            } else {
                keysym::keysym(key.logical_key, key.location, modifiers.shift_key())
            },
            hw_code: keysym::hardware_keycode(key.physical_key),
            modifiers: keysym::modifier_mask(keyboard, num_lock),
            timestamp,
            is_text: !event_string.is_empty() && !chorded,
            event_string,
        }
    }
}

/// A key event as the reporter needs it: the fields of winit's `KeyEvent`
/// (which only winit can build) that say which key it was, plus the
/// `is_synthetic` flag of the `WindowEvent` that carried it.
#[derive(Debug, Clone, Copy)]
pub struct KeyInput<'a> {
    /// The key's place on the keyboard.
    pub physical_key: PhysicalKey,
    /// What the key means with the current layout and shift level.
    pub logical_key: &'a Key,
    /// Which of two keys of the same meaning it was: left or right, main
    /// block or keypad.
    pub location: KeyLocation,
    /// Pressed or released. A repeat is a press.
    pub state: ElementState,
    /// A press the keyboard repeated because the key is held.
    pub repeat: bool,
    /// winit made this event up, for a key already held when focus changed.
    pub synthetic: bool,
}

impl<'a> KeyInput<'a> {
    /// The key of a winit `KeyboardInput`.
    pub fn from_winit(event: &'a winit::event::KeyEvent, is_synthetic: bool) -> Self {
        Self {
            physical_key: event.physical_key,
            logical_key: &event.logical_key,
            location: event.location,
            state: event.state,
            repeat: event.repeat,
            synthetic: is_synthetic,
        }
    }
}

/// What else is true of the keyboard when a key event arrives.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyboardState {
    /// The modifiers held, as winit last reported them.
    pub modifiers: ModifiersState,
    /// Whether Caps Lock is on, as far as the application knows.
    pub caps_lock: bool,
}

/// Where a key lands, as far as reporting it goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyTarget {
    /// An assistive technology is attached to the window the key came to
    /// ([`PlatformWindow::accessibility_active`](crate::PlatformWindow::accessibility_active)).
    pub reader_attached: bool,
    /// The focused widget is a secure text field (it declares
    /// `ImePurpose::Password`, as a `PasswordField` does).
    pub secure_field: bool,
}

/// What the registry said about a reported key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// Only the Linux reporter and the tests answer anything but `Unanswered`.
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
pub(crate) enum ReportOutcome {
    /// A listener took the key for itself; the application must not see it.
    Consumed,
    /// The registry answered, and nobody took the key.
    NotConsumed,
    /// No answer in time, or nobody to ask. The key is the application's.
    Unanswered,
}

/// Where key events are reported. The platform's is the AT-SPI registry on
/// Linux; a test hands the gate one of its own.
pub(crate) trait KeyEventReporter {
    /// Report a key press (or repeat) and wait, briefly, for the answer.
    fn report_press(&mut self, event: &DeviceEvent) -> ReportOutcome;

    /// Report a key release. Nothing waits for its answer.
    fn report_release(&mut self, event: &DeviceEvent);
}

/// Whether a key event goes on to the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyDisposition {
    /// Dispatch it as usual.
    Deliver,
    /// An assistive technology took it: drop it.
    Drop,
}

/// Reports each key to the platform's assistive technology before the
/// application acts on it, and says which keys the application must not see.
/// See the [module documentation](self).
pub struct KeyReportGate {
    reporter: Option<Box<dyn KeyEventReporter>>,
    /// Builds the platform's reporter the first time one is needed.
    start: Option<fn() -> Box<dyn KeyEventReporter>>,
    /// The keys held down, one entry per key from its first press to its
    /// release. A handful at most.
    held: Vec<Hold>,
    /// Whether Num Lock is on, once a key has shown it; `None` until then.
    /// See [`keysym::num_lock_shown`].
    num_lock: Option<bool>,
    /// The origin of [`DeviceEvent::timestamp`].
    epoch: Instant,
}

/// What the gate remembers of one key from its first press to its release.
#[derive(Debug, Clone)]
struct Hold {
    key: PhysicalKey,
    /// Whether any press of this hold (the first, or a repeat) reached the
    /// application. Its release is delivered exactly when one did.
    delivered: bool,
    /// Whether any press of this hold was reported. Its release then is,
    /// whatever window it comes up in.
    reported: bool,
    /// Whether a press of this hold landed in a secure field. Its release
    /// then keeps the character to itself too, wherever focus has gone.
    withheld: bool,
    /// The keysym, text and `is_text` the last reported press of this hold
    /// carried, which its release carries too: winit composes on presses
    /// only (the key that completes a dead-key sequence is `é` down and `e`
    /// up), and Shift can come or go during a hold, while the release is of
    /// the key that went down. Orca pairs a release with its press by them.
    named: Option<(u32, String, bool)>,
}

impl std::fmt::Debug for KeyReportGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyReportGate")
            .field("started", &self.reporter.is_some())
            .finish_non_exhaustive()
    }
}

impl Default for KeyReportGate {
    fn default() -> Self {
        Self::for_platform()
    }
}

impl KeyReportGate {
    /// The gate for this platform. It starts nothing until an assistive
    /// technology is attached; on Windows and macOS it never reports.
    pub fn for_platform() -> Self {
        #[cfg(all(unix, not(target_os = "macos")))]
        let start: Option<fn() -> Box<dyn KeyEventReporter>> = Some(atspi::start_reporter);
        #[cfg(not(all(unix, not(target_os = "macos"))))]
        let start: Option<fn() -> Box<dyn KeyEventReporter>> = None;
        Self {
            reporter: None,
            start,
            held: Vec::new(),
            num_lock: None,
            epoch: Instant::now(),
        }
    }

    /// A gate that reports to `reporter`.
    #[cfg(test)]
    pub(crate) fn with_reporter(reporter: impl KeyEventReporter + 'static) -> Self {
        Self {
            reporter: Some(Box::new(reporter)),
            start: None,
            held: Vec::new(),
            num_lock: None,
            epoch: Instant::now(),
        }
    }

    /// Start the platform's reporter now, so that it is connected before the
    /// first key. Call it once an assistive technology is attached; it does
    /// nothing after the first call.
    pub fn warm_up(&mut self) {
        self.reporter();
    }

    fn reporter(&mut self) -> Option<&mut (dyn KeyEventReporter + 'static)> {
        if self.reporter.is_none()
            && let Some(start) = self.start.take()
        {
            self.reporter = Some(start());
        }
        self.reporter.as_deref_mut()
    }

    /// Report `key` if an assistive technology is attached where it lands
    /// (`target`), and say whether the application gets it.
    ///
    /// A press is dropped when a listener took it. A release goes where the
    /// presses of its hold went, whatever anyone says about the release, and
    /// whether or not a reader is still attached: it is delivered when any
    /// press of the hold was (the first press, or a repeat), so the
    /// application sees a key come up exactly when it saw it go down. A new
    /// press, as opposed to a repeat, starts a new hold. A synthetic event is
    /// neither reported nor dropped.
    pub fn filter(
        &mut self,
        key: &KeyInput<'_>,
        keyboard: KeyboardState,
        target: KeyTarget,
    ) -> KeyDisposition {
        let at_active = target.reader_attached;
        if key.synthetic {
            return KeyDisposition::Deliver;
        }
        let pressed = key.state == ElementState::Pressed;
        if pressed && let Some(on) = keysym::num_lock_shown(key.logical_key, key.location) {
            self.num_lock = Some(on);
        }
        let num_lock = self.num_lock == Some(true);
        let epoch = self.epoch;
        // Read only for a report. Wraps after 49 days, as X server time does.
        let event = |key: &KeyInput<'_>, withhold: bool| {
            let timestamp = epoch.elapsed().as_millis() as u32;
            DeviceEvent::for_key(key, keyboard, num_lock, withhold, timestamp)
        };
        let secure = target.secure_field;
        match key.state {
            ElementState::Pressed => {
                let reporter = if at_active { self.reporter() } else { None };
                let reported = reporter.is_some();
                let mut named = None;
                let outcome = reporter.map_or(ReportOutcome::Unanswered, |reporter| {
                    let press = event(key, secure);
                    let outcome = reporter.report_press(&press);
                    named = Some((press.keysym, press.event_string, press.is_text));
                    outcome
                });
                // The Num Lock key's own event carries the state before it.
                if *key.logical_key == Key::Named(NamedKey::NumLock) && !key.repeat {
                    self.num_lock = self.num_lock.map(|on| !on);
                }
                let delivered = outcome != ReportOutcome::Consumed;
                match self
                    .held
                    .iter_mut()
                    .find(|hold| hold.key == key.physical_key)
                {
                    Some(hold) if key.repeat => {
                        hold.delivered |= delivered;
                        hold.reported |= reported;
                        hold.withheld |= secure;
                        hold.named = named.or(hold.named.take());
                    }
                    Some(hold) => {
                        hold.delivered = delivered;
                        hold.reported = reported;
                        hold.withheld = secure;
                        hold.named = named;
                    }
                    None => self.held.push(Hold {
                        key: key.physical_key,
                        delivered,
                        reported,
                        withheld: secure,
                        named,
                    }),
                }
                if delivered {
                    KeyDisposition::Deliver
                } else {
                    KeyDisposition::Drop
                }
            }
            ElementState::Released => {
                let hold = self
                    .held
                    .iter()
                    .position(|hold| hold.key == key.physical_key)
                    .map(|index| self.held.swap_remove(index));
                // The registry heard the press, so it hears the release, even
                // in a window whose adapter no reader has reached yet:
                // libatspi's legacy device sets the Orca modifier from the one
                // and clears it from the other.
                if (at_active || hold.as_ref().is_some_and(|hold| hold.reported))
                    && let Some(reporter) = self.reporter()
                {
                    let withhold = secure || hold.as_ref().is_some_and(|hold| hold.withheld);
                    let mut release = event(key, withhold);
                    if !withhold
                        && let Some((keysym, text, is_text)) =
                            hold.as_ref().and_then(|hold| hold.named.clone())
                    {
                        release.keysym = keysym;
                        release.event_string = text;
                        release.is_text = is_text;
                    }
                    reporter.report_release(&release);
                }
                // A release with no press seen here (the key went down in
                // another application) is the application's.
                if hold.is_none_or(|hold| hold.delivered) {
                    KeyDisposition::Deliver
                } else {
                    KeyDisposition::Drop
                }
            }
        }
    }
}
