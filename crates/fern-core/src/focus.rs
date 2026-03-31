/// How focus was acquired — used for `:focus-visible` behavior.
/// Only show focus ring when focus was gained via keyboard, not pointer click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusOrigin {
    /// Focus gained via Tab/Shift-Tab keyboard navigation.
    Keyboard,
    /// Focus gained via pointer click.
    Pointer,
    /// Focus set programmatically by the application.
    Programmatic,
}

/// Focus policy for composite widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPolicy {
    /// Default behavior — widget participates in tab navigation if focusable.
    Default,
    /// Widget acts as a focus scope — internal children don't participate in
    /// external tab navigation.
    Scope,
}

impl Default for FocusPolicy {
    fn default() -> Self {
        Self::Default
    }
}
