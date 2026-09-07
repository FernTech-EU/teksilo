// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

use teksilo_tokens::PointerKind;

/// How focus was acquired.
///
/// Read by `:focus-visible` — a focus ring belongs to keyboard and assistive
/// navigation, not to a click — and by anything that has to treat a finger
/// differently from a mouse. The pointer arm carries the device that delivered
/// the focus, because "a pointer focused this" is not one behaviour: a mouse
/// focuses on press, a finger and a pen focus on release, and only a release
/// that lands back on the same focusable counts.
///
/// `#[non_exhaustive]`: matches need a wildcard arm. Most call sites want
/// [`is_pointer`](Self::is_pointer) rather than a match at all.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusOrigin {
    /// Focus gained via Tab/Shift-Tab keyboard navigation, or by any other
    /// keystroke a widget routes into focus.
    Keyboard,
    /// Focus gained by pointing at the widget, with the device that did it.
    Pointer(PointerKind),
    /// Focus set programmatically by the application. Carries no input
    /// modality of its own: a scripted focus leaves the focus ring exactly as
    /// the user's last real interaction left it, which is what
    /// `:focus-visible` does for `element.focus()`.
    Programmatic,
    /// Focus moved by assistive technology — a screen reader's
    /// [`Action::Focus`](accesskit::Action::Focus), or an automation client
    /// standing in for one. Reveals the focus ring: the user is navigating,
    /// they are simply not doing it with a key.
    Accessibility,
}

impl FocusOrigin {
    /// Focus arrived by pointer, from a site that cannot know which device
    /// delivered it.
    ///
    /// A control deriving its own origin from hover or from the tree's
    /// input-modality signal — rather than from the
    /// [`FocusGained`](crate::event::WidgetEvent::FocusGained) it was handed —
    /// knows only that the keyboard was not involved. It says so with
    /// [`PointerKind::Unknown`] instead of naming a device it never saw, so a
    /// consumer reading [`pointer_kind`](Self::pointer_kind) is never told a
    /// finger was a mouse.
    pub const POINTER: Self = Self::Pointer(PointerKind::Unknown);

    /// Whether focus arrived by pointing at the widget.
    ///
    /// The predicate that replaced `== FocusOrigin::Pointer`: a pointer origin
    /// now names its device, so equality against a bare variant no longer
    /// compiles and equality against one device would silently exclude the
    /// others.
    pub const fn is_pointer(self) -> bool {
        matches!(self, Self::Pointer(_))
    }

    /// The device that delivered a pointer focus, or `None` for every other
    /// origin. [`PointerKind::Unknown`] for a widget-side derivation — see
    /// [`POINTER`](Self::POINTER).
    pub const fn pointer_kind(self) -> Option<PointerKind> {
        match self {
            Self::Pointer(kind) => Some(kind),
            _ => None,
        }
    }

    /// Whether this origin reveals the focus ring — the `:focus-visible`
    /// question, answered in one place so the tree's modality signal and any
    /// widget consulting the origin cannot disagree.
    ///
    /// Keyboard and assistive navigation reveal it; a pointer hides it.
    /// [`Programmatic`](Self::Programmatic) answers neither: a scripted focus
    /// declares no modality, so the tree leaves the signal where the last real
    /// interaction put it.
    pub const fn focus_visible(self) -> Option<bool> {
        match self {
            Self::Keyboard | Self::Accessibility => Some(true),
            Self::Pointer(_) => Some(false),
            Self::Programmatic => None,
        }
    }
}

/// Policy for a focus **traversal scope**, declared via the `FocusScope`
/// wrapper widget. Controls what Tab / Shift+Tab does when it reaches the
/// scope's ends.
///
/// A scope groups + scopes the `tab_index` numbering of its descendants:
/// two sibling scopes that both number their children `1, 2, 3` never
/// interleave — each scope is an independent, ordered unit within its
/// parent. This is Teksilo's analogue of Flutter `FocusTraversalGroup` /
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
    /// Use for modal dialogs — the one surface whose pattern (ARIA's Dialog
    /// (Modal)) actually calls for containing focus.
    ///
    /// **Not for popovers or menus.** Those implement Disclosure and Menu,
    /// which mandate the opposite: Tab is an exit gesture there, and the
    /// framework already answers it by dismissing the overlay focus leaves
    /// rather than by trapping focus inside it. Wrapping such an overlay in a
    /// `Cycle` scope defeats that — focus can no longer leave, so the
    /// dismissal never fires and the panel becomes keyboard-inescapable except
    /// via Escape.
    Cycle,
}
