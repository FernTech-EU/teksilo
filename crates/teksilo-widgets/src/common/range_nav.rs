// SPDX-License-Identifier: MPL-2.0
// SPDX-FileCopyrightText: 2026 FernTech

//! What the arrow, page and edge keys mean on a control that holds one
//! bounded number.
//!
//! Seven widgets publish `Role::Slider`, `SpinButton`, `Splitter` or
//! `ScrollBar` over a numeric value — [`Slider`](crate::Slider),
//! [`SpinBox`](crate::SpinBox), [`ScrollBar`](crate::ScrollBar), the colour
//! picker's hue and alpha strips, a `Splitter` handle and a dock resize handle
//! — and each hand-rolled the same eight-key match. They gave **four different
//! answers**: the slider had no paging, the spin box no `Home`/`End`, the
//! strips both, the handles neither. Not one of them looked at the modifiers,
//! so every one of them swallowed `Ctrl+Home` from the application.
//!
//! What each widget keeps is its own arithmetic — a hue wraps at 360°, a scroll
//! bar's page is a viewport it measures geometrically, a handle's extremes are
//! two panes' minimum sizes. What none of them should keep is the decision of
//! *which key means what*, which is the same everywhere the control's topology
//! is the same. That topology is [`RangeKind`].
//!
//! This is the bounded-scalar counterpart of [`list_nav`](super::list_nav), and
//! it is deliberately the same shape: pure functions over a chord, no widget
//! state, and the deviations argued here rather than left as an absent match
//! arm somebody has to notice.
//!
//! ## Why this carries no platform branch
//!
//! Like [`list_nav`](super::list_nav) and unlike [`text_nav`](super::text_nav),
//! there is nothing to branch on. Qt's `QAbstractSlider::keyPressEvent`, GTK4's
//! `GtkScale`, the Win32 trackbar and `<input type=range>` in both WebKit and
//! Blink bind `Home`, `End`, `PageUp` and `PageDown` identically on all three
//! platforms; none of them has a platform guard.
//!
//! macOS again differs, and again Teksilo does not follow it:
//! `StandardKeyBinding.dict` spends `Home`/`End` on
//! `scrollToBeginningOfDocument:` and `scrollToEndOfDocument:`, `PageUp`/
//! `PageDown` on `scrollPageUp:`/`scrollPageDown:`, and `NSStepper` answers
//! only the arrows — so a Mac has no jump-to-extremum key for a slider at all,
//! and on a laptop all four need `Fn`+arrow to press. Reproducing that would
//! ship a slider a keyboard cannot drive to either end. The deviation is
//! deliberate and documented in `docs/range-keyboard.md`.
//!
//! What *is* direction-dependent is the `←`/`→` pair, and that arrives as an
//! explicit `rtl` argument rather than a `cfg!` constant, so both branches stay
//! reachable from one host's test run — the same split
//! [`text_nav`](super::text_nav) and
//! [`list_nav::mac_alias`](super::list_nav::mac_alias) already use.

use teksilo_core::event::{Key, Modifiers};

/// The topology of the control asking — which is what decides whether the page
/// keys and the edge keys bind at all.
///
/// The discriminator is not the widget's name but two questions: does the
/// control have a *range* the user can page through, and does something else
/// already own `Home`/`End`?
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum RangeKind {
    /// A bounded number whose whole range belongs to the keyboard: `Slider`,
    /// the hue and alpha strips, `ScrollBar`. All eight keys bind — Qt's
    /// `QAbstractSlider`, GTK4's `GtkScale`, the Win32 trackbar and
    /// `<input type=range>` all bind all eight, and the W3C ARIA slider
    /// pattern makes `Home`/`End` required and the page keys optional.
    Scalar,
    /// A boundary between two regions, whose "value" is a position: a
    /// `Splitter` handle, a dock resize handle. Arrows and the edge keys bind;
    /// the page keys do **not**. The ARIA window-splitter pattern asks only for
    /// `Home` and `End`, `QSplitterHandle` binds no page keys, and a divider
    /// has no unit a page could be a multiple of — the panes on either side
    /// each own their own `PageDown`.
    Divider,
    /// A number behind an editable text field: `SpinBox`. Arrows and page keys
    /// step it; `Home` and `End` are the **caret's**, and this module never
    /// claims them.
    ///
    /// Qt's `QAbstractSpinBox` routes both to its inner `QLineEdit`, WinUI 3's
    /// `NumberBox` binds only the arrows plus `PageUp`/`PageDown`, Blink's
    /// `HandleKeydownEventForSpinButton` binds only `ArrowUp`/`ArrowDown`, and
    /// Avalonia's `NumericUpDown` and jQuery UI's spinner agree. GTK4's
    /// `GtkSpinButton` is the sole outlier and moves the jump onto
    /// `Ctrl+Home`/`Ctrl+End`; Teksilo does not, because `Ctrl+Home` is the
    /// *document* chord on the two platforms that have one — a `SpinBox` in a
    /// scrollable form would steal it — and typing the number reaches either
    /// end anyway. The W3C ARIA spinbutton pattern does list `Home`/`End` as
    /// required, and concedes in the same breath that a text-editable
    /// spinbutton also honours the platform's single-line text-editing keys.
    TextEditable,
}

/// Which arrow keys the control claims.
///
/// Orthogonal to [`RangeKind`] on purpose: a `Slider` wants all four — and
/// `QAbstractSlider` binds all four whatever the slider's own orientation, so
/// `↑` is not dead on a horizontal volume control — while a vertical
/// `ScrollBar` wants exactly one pair, so it leaves `←`/`→` to the horizontal
/// bar beside it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum RangeAxis {
    /// All four arrows drive the value: `→`/`↑` increase, `←`/`↓` decrease.
    Both,
    /// Only `←`/`→`. `↑`/`↓` fall through.
    Horizontal,
    /// Only `↑`/`↓`. `←`/`→` fall through — which is what leaves them to the
    /// caret in a `SpinBox`.
    Vertical,
}

/// Where a bounded-scalar key sends the value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum RangeMove {
    /// One fine step in the direction pressed. The caller owns the distance —
    /// an arrow step, a `single_step`, a scroll bar's `step_size`, a handle's
    /// keyboard step.
    Step { increase: bool },
    /// One coarse step. The caller owns the distance, and it need not be a
    /// multiple of the fine step: `ScrollBar` resolves it *geometrically* from
    /// the viewport it draws its thumb from, the way
    /// [`NavMove::Page`](super::list_nav::NavMove::Page) is resolved from the
    /// row-offset table. Never produced for [`RangeKind::Divider`].
    Page { increase: bool },
    /// The low end of the control's own range. Usually the minimum; for a
    /// divider that can collapse, the collapsed state *is* its low end. Never
    /// produced for [`RangeKind::TextEditable`].
    ToMin,
    /// The high end of the control's own range. Never produced for
    /// [`RangeKind::TextEditable`].
    ToMax,
}

/// Does this move go towards the **trailing** edge — or, on the vertical
/// axis, **down**?
///
/// [`RangeMove::Step`] and [`RangeMove::Page`] report `increase` in the
/// *value's* terms, and on the vertical axis a value grows **upward**: a
/// slider's `Up` means more. Three kinds of control need the *geometric*
/// direction instead, and deriving it inline is how the vertical pair comes
/// out backwards — it did, in every one of them, before this helper existed:
///
/// - a value that grows towards the trailing edge, like a scroll offset,
///   where `ArrowDown` must add rather than subtract;
/// - geometry anchored to an edge, like a dock side, where which arrow
///   enlarges the side depends on which edge its handle sits on;
/// - a boundary between two panes, like a splitter divider, where the leading
///   pane grows as the divider moves away from it.
///
/// "Trailing" and not "right", because [`range_move`] has already folded the
/// layout direction into `increase`: in a right-to-left window `ArrowLeft`
/// arrives as an increase, and the leading-anchored thing it enlarges is the
/// one on the *right*. So this answers "does the leading side get bigger",
/// which is what all three callers actually ask — not "is x growing".
///
/// `horizontal` is the control's own axis, not the key's.
pub(crate) fn towards_trailing(increase: bool, horizontal: bool) -> bool {
    if horizontal { increase } else { !increase }
}

/// What `key` means with `modifiers` held, on a control of this `kind` claiming
/// these `arrows`, in a layout of this direction — or `None` when the chord is
/// not one this module owns.
///
/// Returns `None` when `Ctrl` / `Alt` / `Super` is held, so `Ctrl+Home` and
/// friends stay available to the application and reach the global
/// Shortcut/Action pipeline. `Shift` is **not** rejected: it is not a distinct
/// chord on any bounded scalar — `QAbstractSlider` and Blink's range input
/// never inspect the modifiers — and Teksilo's single-line text field binds
/// nothing to `Shift+↑`, so rejecting it would make the chord dead rather than
/// deferential. Callers that give `Shift` their own meaning (the date and time
/// editors use it as a ×10 multiplier) read it themselves.
///
/// `rtl` mirrors the `←`/`→` pair and nothing else. `↑`/`↓` are never mirrored
/// — Qt flips only the horizontal pair, on `isRightToLeft()` — and neither are
/// `Home`/`End`/`PageUp`/`PageDown`, which name points in *value* space rather
/// than on screen: `SliderToMinimum` is the minimum in either direction.
///
/// Takes no platform convention on purpose; see the module documentation.
pub(crate) fn range_move(
    key: Key,
    modifiers: Modifiers,
    kind: RangeKind,
    arrows: RangeAxis,
    rtl: bool,
) -> Option<RangeMove> {
    // An accelerator-modified chord belongs to the application. Every one of
    // these widgets used to answer it, so `Ctrl+Home` drove a slider to its
    // minimum and reported the key handled, and no ancestor ever saw it. Same
    // rule, and the same reason, as `list_nav::tree_chord` and `MenuList`'s
    // type-ahead guard.
    if modifiers.ctrl() || modifiers.alt() || modifiers.super_key() {
        return None;
    }
    let (increase_key, decrease_key) = if rtl {
        (Key::ArrowLeft, Key::ArrowRight)
    } else {
        (Key::ArrowRight, Key::ArrowLeft)
    };
    let horizontal = matches!(arrows, RangeAxis::Both | RangeAxis::Horizontal);
    let vertical = matches!(arrows, RangeAxis::Both | RangeAxis::Vertical);
    match key {
        k if horizontal && k == increase_key => Some(RangeMove::Step { increase: true }),
        k if horizontal && k == decrease_key => Some(RangeMove::Step { increase: false }),
        Key::ArrowUp if vertical => Some(RangeMove::Step { increase: true }),
        Key::ArrowDown if vertical => Some(RangeMove::Step { increase: false }),
        Key::PageUp if kind != RangeKind::Divider => Some(RangeMove::Page { increase: true }),
        Key::PageDown if kind != RangeKind::Divider => Some(RangeMove::Page { increase: false }),
        Key::Home if kind != RangeKind::TextEditable => Some(RangeMove::ToMin),
        Key::End if kind != RangeKind::TextEditable => Some(RangeMove::ToMax),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::RangeAxis::{Both, Horizontal, Vertical};
    use super::RangeKind::{Divider, Scalar, TextEditable};
    use super::*;

    const NONE: Modifiers = Modifiers::NONE;
    const SHIFT: Modifiers = Modifiers::SHIFT;
    const ALT: Modifiers = Modifiers::ALT;
    const SUPER: Modifiers = Modifiers::SUPER;
    const CTRL: Modifiers = Modifiers::CTRL;
    /// The platform accelerator — ⌘ on macOS, Ctrl elsewhere. Written this way
    /// so the assertions below hold on every host.
    const CMD: Modifiers = Modifiers::COMMAND;

    /// Every key this module has an opinion about, so the rejection tests can
    /// sweep the whole family rather than a hand-picked subset.
    const FAMILY: [Key; 8] = [
        Key::ArrowLeft,
        Key::ArrowRight,
        Key::ArrowUp,
        Key::ArrowDown,
        Key::PageUp,
        Key::PageDown,
        Key::Home,
        Key::End,
    ];

    const ALL_KINDS: [RangeKind; 3] = [Scalar, Divider, TextEditable];

    fn mv(key: Key, kind: RangeKind, arrows: RangeAxis) -> Option<RangeMove> {
        range_move(key, NONE, kind, arrows, false)
    }

    #[test]
    fn every_kind_steps_on_its_own_arrows() {
        for kind in ALL_KINDS {
            assert_eq!(
                mv(Key::ArrowRight, kind, Both),
                Some(RangeMove::Step { increase: true })
            );
            assert_eq!(
                mv(Key::ArrowUp, kind, Both),
                Some(RangeMove::Step { increase: true })
            );
            assert_eq!(
                mv(Key::ArrowLeft, kind, Both),
                Some(RangeMove::Step { increase: false })
            );
            assert_eq!(
                mv(Key::ArrowDown, kind, Both),
                Some(RangeMove::Step { increase: false })
            );
        }
    }

    #[test]
    fn a_scalar_binds_the_whole_family() {
        assert_eq!(mv(Key::Home, Scalar, Both), Some(RangeMove::ToMin));
        assert_eq!(mv(Key::End, Scalar, Both), Some(RangeMove::ToMax));
        assert_eq!(
            mv(Key::PageUp, Scalar, Both),
            Some(RangeMove::Page { increase: true })
        );
        assert_eq!(
            mv(Key::PageDown, Scalar, Both),
            Some(RangeMove::Page { increase: false })
        );
    }

    #[test]
    fn a_divider_has_no_page() {
        // The ARIA window-splitter pattern asks only for `Home` and `End`, both
        // optional, and `QSplitterHandle` binds no page keys: a divider has no
        // unit a page could be a multiple of.
        assert_eq!(mv(Key::PageUp, Divider, Horizontal), None);
        assert_eq!(mv(Key::PageDown, Divider, Horizontal), None);
        assert_eq!(mv(Key::Home, Divider, Horizontal), Some(RangeMove::ToMin));
        assert_eq!(mv(Key::End, Divider, Horizontal), Some(RangeMove::ToMax));
    }

    #[test]
    fn a_text_editable_leaves_the_edges_to_the_caret() {
        // Qt's `QAbstractSpinBox` routes both to its inner `QLineEdit`; WinUI's
        // `NumberBox`, Blink, Avalonia and jQuery UI bind neither. The page keys
        // still step, which is what all of them except Blink do.
        assert_eq!(mv(Key::Home, TextEditable, Vertical), None);
        assert_eq!(mv(Key::End, TextEditable, Vertical), None);
        assert_eq!(
            mv(Key::PageUp, TextEditable, Vertical),
            Some(RangeMove::Page { increase: true })
        );
        assert_eq!(
            mv(Key::PageDown, TextEditable, Vertical),
            Some(RangeMove::Page { increase: false })
        );
    }

    #[test]
    fn a_text_editable_never_reaches_an_extremum() {
        // Swept rather than spot-checked, because this is the assertion that
        // licenses `SpinBox` treating `ToMin` / `ToMax` as unreachable.
        for key in FAMILY {
            for mods in [NONE, SHIFT] {
                for arrows in [Both, Horizontal, Vertical] {
                    let got = range_move(key, mods, TextEditable, arrows, false);
                    assert!(
                        !matches!(got, Some(RangeMove::ToMin) | Some(RangeMove::ToMax)),
                        "{key:?} with {mods:?} on {arrows:?} reached an extremum: {got:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_axis_decides_which_arrows_are_claimed() {
        assert_eq!(mv(Key::ArrowLeft, Scalar, Vertical), None);
        assert_eq!(mv(Key::ArrowRight, Scalar, Vertical), None);
        assert_eq!(mv(Key::ArrowUp, Scalar, Horizontal), None);
        assert_eq!(mv(Key::ArrowDown, Scalar, Horizontal), None);
        // `Both` claims all four.
        for key in [
            Key::ArrowLeft,
            Key::ArrowRight,
            Key::ArrowUp,
            Key::ArrowDown,
        ] {
            assert!(mv(key, Scalar, Both).is_some(), "{key:?} on Both");
        }
    }

    #[test]
    fn rtl_mirrors_the_horizontal_pair_and_nothing_else() {
        // Qt flips on `isRightToLeft()`, and only the horizontal pair.
        assert_eq!(
            range_move(Key::ArrowLeft, NONE, Scalar, Both, true),
            Some(RangeMove::Step { increase: true })
        );
        assert_eq!(
            range_move(Key::ArrowRight, NONE, Scalar, Both, true),
            Some(RangeMove::Step { increase: false })
        );
        // The vertical arrows, the page keys and the edges name points in value
        // space, not on screen, so the direction cannot touch them.
        for key in [
            Key::ArrowUp,
            Key::ArrowDown,
            Key::PageUp,
            Key::PageDown,
            Key::Home,
            Key::End,
        ] {
            assert_eq!(
                range_move(key, NONE, Scalar, Both, true),
                range_move(key, NONE, Scalar, Both, false),
                "{key:?} must not mirror"
            );
        }
    }

    #[test]
    fn an_accelerator_chord_belongs_to_the_application() {
        // Before this module every one of these widgets answered `Ctrl+Home`
        // and reported the key handled, so the chord never reached the global
        // Shortcut/Action pipeline.
        for mods in [CTRL, ALT, SUPER, CMD] {
            for kind in ALL_KINDS {
                for key in FAMILY {
                    assert_eq!(
                        range_move(key, mods, kind, Both, false),
                        None,
                        "{key:?} with {mods:?} on {kind:?} must fall through"
                    );
                }
            }
        }
    }

    #[test]
    fn a_shifted_chord_is_still_the_chord() {
        // `Shift` is not a distinct chord on a bounded scalar, and the single-
        // line field binds nothing to `Shift+↑`, so rejecting it would make the
        // chord dead rather than deferential.
        for kind in ALL_KINDS {
            for key in FAMILY {
                assert_eq!(
                    range_move(key, SHIFT, kind, Both, false),
                    range_move(key, NONE, kind, Both, false),
                    "{key:?} on {kind:?}"
                );
            }
        }
    }

    #[test]
    fn the_geometric_direction_inverts_only_on_the_vertical_axis() {
        // `Right` and `Up` both "increase", because a slider's value grows
        // upward. A scroll offset, a dock side and a splitter divider do not,
        // and reading `increase` as a geometric direction is how all three
        // ended up moving the wrong way on their vertical arrows.
        assert!(towards_trailing(true, true), "an increase is trailing-ward");
        assert!(!towards_trailing(false, true), "a decrease is leading-ward");
        assert!(
            towards_trailing(false, false),
            "Down is trailing-ward even though it decreases the value"
        );
        assert!(
            !towards_trailing(true, false),
            "Up is leading-ward even though it increases the value"
        );
    }

    #[test]
    fn trailing_is_not_a_synonym_for_rightward() {
        // The layout direction is folded into `increase` upstream, so under
        // RTL the trailing-ward arrow is the one pointing *left*. Callers ask
        // "does the leading side get bigger", not "is x growing" — which is
        // why the helper is not named for the screen axis.
        let rtl_increase = range_move(Key::ArrowLeft, NONE, Divider, Horizontal, true);
        assert_eq!(rtl_increase, Some(RangeMove::Step { increase: true }));
        assert!(towards_trailing(true, true));
    }

    #[test]
    fn keys_outside_the_family_are_not_ours() {
        for key in [
            Key::Space,
            Key::Enter,
            Key::Escape,
            Key::Tab,
            Key::A,
            Key::Character('*'),
        ] {
            for kind in ALL_KINDS {
                assert_eq!(range_move(key, NONE, kind, Both, false), None, "{key:?}");
            }
        }
    }
}
