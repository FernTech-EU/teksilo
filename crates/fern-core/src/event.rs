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
    A, B, C, D, E, F, G, H, I, J, K, L, M,
    N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    // Function keys
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
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
