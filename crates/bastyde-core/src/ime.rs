//! Input-method-editor (IME) descriptors attached to widget nodes.
//!
//! A focusable node carries an optional [`ImeContext`]. Its **presence**
//! declares "this node is a text-input surface" — the platform layer enables
//! the OS input method while the node is focused. Its **absence** (the
//! default for every node) means no OS IME: enabling IME changes how text
//! arrives (printable text routes through `Ime::Commit` and `KeyboardInput`
//! is suppressed during preedit), so the safe, common-case default is off.
//!
//! This module is deliberately winit-free — [`ImePurpose`] mirrors winit's
//! enum of the same name so `bastyde-core` stays decoupled from the windowing
//! backend; `bastyde-app` maps between the two at the platform boundary.

/// Hint describing what an IME-enabled field is used for. Lets the platform
/// optimize the input method — e.g. suppress the learning dictionary /
/// candidate history for passwords, or surface terminal-specific keys.
///
/// Mirrors `winit::window::ImePurpose`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImePurpose {
    /// No special hint (ordinary text entry).
    #[default]
    Normal,
    /// Password entry. The platform suppresses IME history/learning; the
    /// widget masks the preedit and never exposes composing text to AT.
    Password,
    /// Terminal entry (e.g. extra on-screen-keyboard keys on Wayland).
    Terminal,
}

/// Per-node IME descriptor. Presence = "this focusable node is a text-input
/// surface" (OS IME enabled while focused); absence (the node default) = no
/// OS IME. Carried on `WidgetNode`, set via `WidgetBuilder::ime_input`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImeContext {
    /// Purpose hint forwarded to the platform IME.
    pub purpose: ImePurpose,
}

impl ImeContext {
    /// An ordinary text-input surface (`ImePurpose::Normal`).
    pub fn text() -> Self {
        Self {
            purpose: ImePurpose::Normal,
        }
    }

    /// A password surface (`ImePurpose::Password`). IME stays enabled so
    /// non-Latin users can compose passwords; the widget is responsible for
    /// masking the preedit on screen and hiding it from assistive tech.
    pub fn password() -> Self {
        Self {
            purpose: ImePurpose::Password,
        }
    }
}
