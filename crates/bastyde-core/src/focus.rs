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

/// Policy for a focus **traversal scope**, declared via the `FocusScope`
/// wrapper widget. Controls what Tab / Shift+Tab does when it reaches the
/// scope's ends.
///
/// A scope groups + scopes the `tab_index` numbering of its descendants:
/// two sibling scopes that both number their children `1, 2, 3` never
/// interleave — each scope is an independent, ordered unit within its
/// parent. This is Bastyde's analogue of Flutter `FocusTraversalGroup` /
/// WPF `KeyboardNavigation.TabNavigation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraversalScopePolicy {
    /// Tab flows *out* of the scope at its ends into the enclosing scope's
    /// next member. The scope groups `tab_index` numbering without trapping
    /// focus — use for logical regions in a continuous Tab order (e.g. dock
    /// panels, where each panel numbers its own controls without colliding
    /// with sibling panels).
    Continue,
    /// Tab *wraps* within the scope and never exits via keyboard navigation.
    /// Use for modal dialogs, popovers, and any overlay that must trap focus
    /// until explicitly dismissed.
    Cycle,
}
