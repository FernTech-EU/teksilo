use fern_canvas::{Point, Rect};

use crate::gesture::GestureEvent;

/// Pointer button identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
}

/// Keyboard key identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Space,
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
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
    // Other
    Character(char),
}

/// Keyboard modifier state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
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

/// Events dispatched to widgets.
#[derive(Debug, Clone)]
pub enum WidgetEvent {
    PointerDown {
        position: Point,
        button: PointerButton,
    },
    PointerUp {
        position: Point,
        button: PointerButton,
    },
    PointerMove {
        position: Point,
    },
    PointerEnter,
    PointerLeave,
    Scroll {
        delta: ScrollDelta,
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
