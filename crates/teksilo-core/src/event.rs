// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use teksilo_canvas::{Point, Rect};

use crate::gesture::GestureEvent;

/// Pointer button identifiers.
///
/// `Forward` and `Back` correspond to the auxiliary mouse buttons (mouse
/// 4 / mouse 5) typically labelled "browser back / forward". Platforms
/// that don't have those buttons simply never emit them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerButton {
    /// Left-click (or main-action button on left-handed mice).
    Primary,
    /// Right-click.
    Secondary,
    /// Middle / wheel-click.
    Middle,
    /// "Back" auxiliary button (mouse 4 on most 5-button mice). Often
    /// bound to "navigate back" in browsers.
    Back,
    /// "Forward" auxiliary button (mouse 5). Often bound to "navigate
    /// forward".
    Forward,
}

/// Set of pointer buttons a gesture recognizer is configured to fire
/// for. Used by the four click-style recognizers (`TapRecognizer`,
/// `DoubleTapRecognizer`, `TripleTapRecognizer`, `LongPressRecognizer`)
/// and the matching widget-level builders (`accept_tap_buttons`, …).
///
/// Default for every recognizer is [`ButtonMask::PRIMARY`] — left-click
/// only — which matches the user's expectation for a "tap" and keeps
/// right-click free to open a context menu without spuriously
/// activating the widget. Use [`ButtonMask::ALL`] or a hand-built
/// `PRIMARY | SECONDARY` etc. to opt into broader button sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ButtonMask(u8);

impl ButtonMask {
    /// Empty mask — no buttons accepted.
    pub const NONE: Self = Self(0);
    /// Left-click on most desktop pointing devices.
    pub const PRIMARY: Self = Self(1 << 0);
    /// Right-click on most desktop pointing devices.
    pub const SECONDARY: Self = Self(1 << 1);
    /// Middle / wheel-click.
    pub const MIDDLE: Self = Self(1 << 2);
    /// "Back" auxiliary button (mouse 4).
    pub const BACK: Self = Self(1 << 3);
    /// "Forward" auxiliary button (mouse 5).
    pub const FORWARD: Self = Self(1 << 4);
    /// All buttons currently representable by [`PointerButton`].
    pub const ALL: Self = Self(0b0001_1111);

    /// `true` when the mask contains the given button.
    pub const fn contains(self, button: PointerButton) -> bool {
        let bit = match button {
            PointerButton::Primary => 1 << 0,
            PointerButton::Secondary => 1 << 1,
            PointerButton::Middle => 1 << 2,
            PointerButton::Back => 1 << 3,
            PointerButton::Forward => 1 << 4,
        };
        self.0 & bit != 0
    }

    /// `true` when no buttons are accepted.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Union — accept any button in either mask.
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Intersection — accept only buttons present in both masks.
    pub const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }
}

impl From<PointerButton> for ButtonMask {
    fn from(button: PointerButton) -> Self {
        match button {
            PointerButton::Primary => Self::PRIMARY,
            PointerButton::Secondary => Self::SECONDARY,
            PointerButton::Middle => Self::MIDDLE,
            PointerButton::Back => Self::BACK,
            PointerButton::Forward => Self::FORWARD,
        }
    }
}

impl<const N: usize> From<[PointerButton; N]> for ButtonMask {
    fn from(buttons: [PointerButton; N]) -> Self {
        let mut mask = Self::NONE;
        let mut i = 0;
        while i < N {
            mask = mask.union(ButtonMask::from(buttons[i]));
            i += 1;
        }
        mask
    }
}

impl std::ops::BitOr for ButtonMask {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl std::ops::BitAnd for ButtonMask {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        self.intersection(rhs)
    }
}

impl std::ops::BitOrAssign for ButtonMask {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl std::ops::BitAndAssign for ButtonMask {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl Default for ButtonMask {
    fn default() -> Self {
        Self::PRIMARY
    }
}

/// Keyboard key identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Key {
    Space,
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    Insert,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    // Letters
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    // Function keys
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F20,
    F21,
    F22,
    F23,
    F24,
    // Other
    /// Caps Lock. Delivered as a discrete key press/release (winit's
    /// `ModifiersState` does not carry lock state), so consumers that
    /// need the *active* lock state track it themselves on the
    /// key-down edge. See `WindowState::caps_lock`.
    CapsLock,
    Character(char),
}

impl Key {
    /// Returns the character this key represents, if any.
    /// Maps `Key::A`..`Key::Z` to `'a'`..`'z'` (lowercase) and
    /// `Key::Character(ch)` to `ch`.
    pub fn to_char(&self) -> Option<char> {
        match self {
            Key::A => Some('a'),
            Key::B => Some('b'),
            Key::C => Some('c'),
            Key::D => Some('d'),
            Key::E => Some('e'),
            Key::F => Some('f'),
            Key::G => Some('g'),
            Key::H => Some('h'),
            Key::I => Some('i'),
            Key::J => Some('j'),
            Key::K => Some('k'),
            Key::L => Some('l'),
            Key::M => Some('m'),
            Key::N => Some('n'),
            Key::O => Some('o'),
            Key::P => Some('p'),
            Key::Q => Some('q'),
            Key::R => Some('r'),
            Key::S => Some('s'),
            Key::T => Some('t'),
            Key::U => Some('u'),
            Key::V => Some('v'),
            Key::W => Some('w'),
            Key::X => Some('x'),
            Key::Y => Some('y'),
            Key::Z => Some('z'),
            Key::Character(ch) => Some(*ch),
            _ => None,
        }
    }
}

/// Keyboard modifier state.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub struct Modifiers {
    bits: u8,
}

impl Modifiers {
    pub const NONE: Modifiers = Modifiers { bits: 0 };
    pub const CTRL: Modifiers = Modifiers { bits: 1 };
    pub const SHIFT: Modifiers = Modifiers { bits: 2 };
    pub const ALT: Modifiers = Modifiers { bits: 4 };
    pub const SUPER: Modifiers = Modifiers { bits: 8 };

    pub fn empty() -> Self {
        Self::NONE
    }

    pub fn ctrl(self) -> bool {
        self.bits & 1 != 0
    }

    pub fn shift(self) -> bool {
        self.bits & 2 != 0
    }

    pub fn alt(self) -> bool {
        self.bits & 4 != 0
    }

    pub fn super_key(self) -> bool {
        self.bits & 8 != 0
    }
}

impl std::fmt::Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Key::Space => f.write_str("Space"),
            Key::Enter => f.write_str("Enter"),
            Key::Escape => f.write_str("Esc"),
            Key::Tab => f.write_str("Tab"),
            Key::Backspace => f.write_str("Backspace"),
            Key::Delete => f.write_str("Del"),
            Key::Insert => f.write_str("Ins"),
            Key::ArrowUp => f.write_str("Up"),
            Key::ArrowDown => f.write_str("Down"),
            Key::ArrowLeft => f.write_str("Left"),
            Key::ArrowRight => f.write_str("Right"),
            Key::Home => f.write_str("Home"),
            Key::End => f.write_str("End"),
            Key::PageUp => f.write_str("PageUp"),
            Key::PageDown => f.write_str("PageDown"),
            Key::A => f.write_str("A"),
            Key::B => f.write_str("B"),
            Key::C => f.write_str("C"),
            Key::D => f.write_str("D"),
            Key::E => f.write_str("E"),
            Key::F => f.write_str("F"),
            Key::G => f.write_str("G"),
            Key::H => f.write_str("H"),
            Key::I => f.write_str("I"),
            Key::J => f.write_str("J"),
            Key::K => f.write_str("K"),
            Key::L => f.write_str("L"),
            Key::M => f.write_str("M"),
            Key::N => f.write_str("N"),
            Key::O => f.write_str("O"),
            Key::P => f.write_str("P"),
            Key::Q => f.write_str("Q"),
            Key::R => f.write_str("R"),
            Key::S => f.write_str("S"),
            Key::T => f.write_str("T"),
            Key::U => f.write_str("U"),
            Key::V => f.write_str("V"),
            Key::W => f.write_str("W"),
            Key::X => f.write_str("X"),
            Key::Y => f.write_str("Y"),
            Key::Z => f.write_str("Z"),
            Key::F1 => f.write_str("F1"),
            Key::F2 => f.write_str("F2"),
            Key::F3 => f.write_str("F3"),
            Key::F4 => f.write_str("F4"),
            Key::F5 => f.write_str("F5"),
            Key::F6 => f.write_str("F6"),
            Key::F7 => f.write_str("F7"),
            Key::F8 => f.write_str("F8"),
            Key::F9 => f.write_str("F9"),
            Key::F10 => f.write_str("F10"),
            Key::F11 => f.write_str("F11"),
            Key::F12 => f.write_str("F12"),
            Key::F13 => f.write_str("F13"),
            Key::F14 => f.write_str("F14"),
            Key::F15 => f.write_str("F15"),
            Key::F16 => f.write_str("F16"),
            Key::F17 => f.write_str("F17"),
            Key::F18 => f.write_str("F18"),
            Key::F19 => f.write_str("F19"),
            Key::F20 => f.write_str("F20"),
            Key::F21 => f.write_str("F21"),
            Key::F22 => f.write_str("F22"),
            Key::F23 => f.write_str("F23"),
            Key::F24 => f.write_str("F24"),
            Key::CapsLock => f.write_str("CapsLock"),
            Key::Character(c) => write!(f, "{}", c.to_uppercase()),
        }
    }
}

impl std::fmt::Display for Modifiers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.ctrl() {
            f.write_str("Ctrl+")?;
        }
        if self.alt() {
            f.write_str("Alt+")?;
        }
        if self.shift() {
            f.write_str("Shift+")?;
        }
        if self.super_key() {
            f.write_str("Super+")?;
        }
        Ok(())
    }
}

impl std::ops::BitOr for Modifiers {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Modifiers {
            bits: self.bits | rhs.bits,
        }
    }
}

/// Scroll delta from mouse wheel or trackpad.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollDelta {
    /// Line-based scrolling (mouse wheel).
    Lines { x: f32, y: f32 },
    /// Pixel-based scrolling (trackpad).
    Pixels { x: f32, y: f32 },
}

/// Where a [`WidgetEvent::ScrollIntoView`] target should come to rest on the
/// scroll container's vertical axis.
///
/// The horizontal axis is always revealed minimally — a fraction only has an
/// obvious meaning for the axis the request is *about*, and pinning a caret
/// vertically must not yank a horizontally-scrolled view sideways.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollAlign {
    /// Scroll the least amount that makes the target fully visible, and not at
    /// all when it already is. This is what focus-driven reveals and
    /// [`EventContext::ensure_visible`](crate::widget::EventContext::ensure_visible)
    /// use, and it is the behaviour every scroll container had before
    /// alignment existed.
    Minimal,
    /// Pin the target at `f` of the way down the viewport — `0.0` flush with
    /// the top, `0.5` centred, `1.0` flush with the bottom — **whether or not
    /// it is already visible**. Being unconditional is the whole point: a
    /// typewriter-scrolling caret that only moved the view when it fell off
    /// the edge would not be pinned at all.
    ///
    /// The container still clamps to its scroll range, so a target near the
    /// start or end of the content comes to rest as close to `f` as the range
    /// allows. See [`ScrollArea::scroll_past_end`] for buying range past the
    /// end of the content so the last line can still reach the pin.
    ///
    /// [`ScrollArea::scroll_past_end`]: https://docs.rs/teksilo-widgets
    Fraction(f32),
}

/// Whether a [`WidgetEvent::ScrollIntoView`] should jump or glide.
///
/// Split out from the container's own `smooth_scrolling` setting because the
/// right answer depends on the *request*, not the container: a caret pinned on
/// every keystroke must snap (animating it is what produces the "screen
/// bouncing" typewriter-mode users complain about in other editors), while the
/// same container gliding for a page-down or a search hit reads as polish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollMotion {
    /// Jump straight to the target offset.
    Instant,
    /// Animate to the target offset, if the container has smooth scrolling
    /// enabled. Containers with `smooth_scrolling(false)` still jump.
    Smooth,
}

/// Events dispatched to widgets.
#[derive(Debug, Clone)]
pub enum WidgetEvent {
    PointerDown {
        position: Point,
        button: PointerButton,
        modifiers: Modifiers,
    },
    PointerUp {
        position: Point,
        button: PointerButton,
        modifiers: Modifiers,
    },
    PointerMove {
        position: Point,
    },
    PointerEnter,
    PointerLeave,
    Scroll {
        delta: ScrollDelta,
        /// Modifier keys held at the time of the scroll event.
        /// Defaults to `Modifiers::NONE` for synthesized events
        /// (tests, keyboard-driven scroll requests). Real-platform
        /// scroll events populate this from the platform's tracked
        /// modifier state — apps detect Ctrl-wheel-to-zoom by
        /// inspecting `modifiers.ctrl()`.
        modifiers: Modifiers,
    },
    KeyDown {
        key: Key,
        modifiers: Modifiers,
        text: Option<String>,
    },
    KeyUp {
        key: Key,
        modifiers: Modifiers,
    },
    ImeComposition {
        text: String,
        cursor: Option<std::ops::Range<usize>>,
    },
    ImeCommit {
        text: String,
    },
    FocusGained {
        origin: crate::focus::FocusOrigin,
    },
    FocusLost,
    AccessAction {
        action: accesskit::Action,
        target: Option<crate::widget_id::WidgetId>,
        /// Raw AccessKit NodeId from the original `ActionRequest`.
        /// May be a synthetic (widget-emitted child) NodeId — use
        /// `crate::accessibility::is_synthetic` to distinguish it
        /// from a widget-derived NodeId. The widget that registered
        /// the parent (retrieved via `tree.widget_for_synthetic`)
        /// is the one set in `target`.
        target_node: accesskit::NodeId,
        /// Payload carried by the `ActionRequest`. For
        /// `Action::SetTextSelection` this is
        /// `ActionData::SetTextSelection(TextSelection)`, for
        /// `Action::SetValue` it's `ActionData::Value(Box<str>)`,
        /// for scroll actions it carries scroll offsets, etc.
        /// Widgets that declare these actions must read the payload
        /// to honour screen-reader-initiated requests.
        data: Option<accesskit::ActionData>,
    },
    /// Dispatched by the framework to a clipping ancestor when a child
    /// gains focus but is outside the viewport. The scroll area adjusts
    /// its offset to make the target bounds visible, with an optional
    /// margin around the target.
    ScrollIntoView {
        target_bounds: Rect,
        /// Extra margin (in logical pixels) to keep around the target
        /// when scrolling it into view. Defaults to 0.0.
        margin: f32,
        /// Where the target should end up on the scroll container's
        /// **vertical** axis. [`ScrollAlign::Minimal`] (the default, and what
        /// every focus-driven reveal uses) only scrolls when the target is not
        /// already fully visible; [`ScrollAlign::Fraction`] *pins* it to a
        /// fixed height in the viewport whether or not it was already visible.
        align: ScrollAlign,
        /// Whether the container should jump to the new offset or glide to it.
        /// See [`ScrollMotion`].
        motion: ScrollMotion,
        /// Optional back-channel for the handling scroll container to report
        /// how far it actually scrolled (`(dx, dy)` in content pixels). When
        /// several nested scroll containers must each reveal the same target,
        /// the ancestor walk (`scroll_rect_into_view`) reads this after
        /// dispatching to an inner container and shifts `target_bounds` by the
        /// negated delta before asking the next (outer) one — so the outer sees
        /// where the target will land once the inner's (deferred) scroll
        /// applies, not its pre-scroll position. `None` disables reporting (the
        /// nested-reveal refinement is unavailable). A handler that ignores it
        /// still works for the common single-container case.
        ///
        /// `Arc<Mutex<..>>` (not `Rc<Cell<..>>`) so `WidgetEvent` stays `Send`
        /// — some events are posted across threads. This one is only ever
        /// touched on the dispatch thread, so the lock is always uncontended.
        applied_scroll: Option<std::sync::Arc<std::sync::Mutex<teksilo_canvas::Point>>>,
    },
    /// A recognized gesture event, routed through the same preview/bubble system.
    Gesture {
        gesture: GestureEvent,
    },
}

/// The result of handling an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventResponse {
    /// The event was handled; stop propagation.
    Handled,
    /// The event was not handled; let it bubble.
    Ignored,
}
