// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusPolicy {
    /// Default behavior — widget participates in tab navigation if focusable.
    #[default]
    Default,
    /// Widget acts as a focus scope — internal children don't participate in
    /// external tab navigation.
    Scope,
}
